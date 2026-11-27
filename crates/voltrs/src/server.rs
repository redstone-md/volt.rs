//! Request dispatch: static assets, actions, data, API handlers and rendered pages.

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use http::{HeaderValue, Method, StatusCode, header};
use http_body_util::{BodyExt, Either, Full, Limited};
use hyper::body::Incoming;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::app::Prerendered;
use crate::assets::{StaticAssets, compressible, safe_join, serve_public};
use crate::cache::{self, CachedPage, Lookup, PageCache};
use crate::config::Config;
use crate::context::{Ctx, RequestParts, State};
use crate::dev::{EventStream, Hub};
use crate::document::{Document, ERROR_ROUTE, NOT_FOUND_ROUTE, PageState, Site, SlotState, error_overlay};
use crate::error::{Error, ErrorKind, Result};
use crate::flash::ActionFlash;
use crate::form::FormData;
use crate::handlers::{self, Outcome, ScriptRequest, ScriptStream, collect, origin};
use crate::log;
use crate::manifest::{Manifest, RouteEntry, STATIC_PREFIX, SlotRoute};
use crate::matcher::{ConditionKind, PathMatcher, Verdict};
use crate::metadata::MetadataAssets;
use crate::registry::{self, ActionDef, ApiDef, BoxFuture, LoaderDef, Payload};
use crate::response::{IntoResponse, Response};
use crate::router::{Params, RoutePattern, Router, canonical_path};
use crate::seo::{self, MetadataRoute, Robots, SitemapEntry};
use crate::slots::Slots;
use crate::ssr::{Engine, EngineOptions, FetchOptions, FetchRequest, Fetcher, StreamEvent};
use crate::stream::PageStream;

/// Header of client navigations naming the route the client navigates from, which decides interceptions.
const FROM_HEADER: &str = "x-volt-from";

pub type Middleware = Arc<dyn Fn(Ctx) -> BoxFuture<Option<Response>> + Send + Sync>;
type HyperResponse = hyper::Response<Either<Full<Bytes>, Either<EventStream, Either<PageStream, ScriptStream>>>>;

/// A loaded build: manifest, page routes, intercepting routes, slots, static assets and `app/` metadata files.
struct Build {
    manifest: Manifest,
    pages: Router<usize>,
    /// Routes that only exist in slots and render the `default` page of their layout, tried after `pages`.
    fallbacks: Router<usize>,
    /// Intercepting routes by the pattern they intercept.
    intercepts: Router<usize>,
    slots: Slots,
    assets: StaticAssets,
    metadata: MetadataAssets,
    /// Route handlers (`route.ts`) by their pattern.
    handlers: Router<usize>,
    /// `GET` responses of `force-static` route handlers, by handler index.
    prerendered: HashMap<usize, Response>,
    /// Paths `middleware.ts` runs on, when there is one.
    middleware: Option<PathMatcher>,
}

impl Build {
    fn entry(&self, path: &str) -> Option<&RouteEntry> {
        let (_, index, _) = self.pages.find(path).or_else(|| self.fallbacks.find(path))?;
        Some(&self.manifest.routes[*index])
    }

    /// The routes that `keep` selects, by their own pattern.
    fn table(manifest: &Manifest, keep: impl Fn(&RouteEntry) -> bool) -> Router<usize> {
        manifest.routes.iter().enumerate().filter(|(_, r)| keep(r)).map(|(i, r)| (r.id.clone(), i)).collect()
    }

    fn load(out_dir: &std::path::Path) -> Result<Self> {
        let manifest = Manifest::load(out_dir)
            .map_err(|e| Error::internal(format!("no build found in {} ({e}); run `volt build`", out_dir.display())))?;
        let pages = Self::table(&manifest, |r| r.is_page() && r.fallback.is_none());
        let fallbacks = Self::table(&manifest, |r| r.is_page() && r.fallback.is_some());
        let intercepts = manifest
            .routes
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.intercept.as_ref().map(|intercept| (intercept.target.clone(), i)))
            .collect();
        let slots = Slots::new(&manifest.slots);
        let assets = StaticAssets::load(&out_dir.join("static"), manifest.dev);
        let metadata = MetadataAssets::load(out_dir, &manifest.metadata);
        let handlers = manifest.handlers.iter().enumerate().map(|(i, h)| (h.id.clone(), i)).collect();
        let prerendered = Self::prerendered_handlers(&manifest, out_dir);
        let middleware = match &manifest.middleware {
            Some(entry) => Some(PathMatcher::new(&entry.matcher).map_err(Error::internal)?),
            None => None,
        };
        Ok(Self { manifest, pages, fallbacks, intercepts, slots, assets, metadata, handlers, prerendered, middleware })
    }

    fn prerendered_handlers(manifest: &Manifest, out_dir: &std::path::Path) -> HashMap<usize, Response> {
        let mut out = HashMap::new();
        for (index, handler) in manifest.handlers.iter().enumerate() {
            let Some(written) = &handler.prerendered else { continue };
            let Ok(body) = std::fs::read(out_dir.join(&written.file)) else {
                log::warn(&format!("missing prerendered response of {}", handler.id));
                continue;
            };
            let mut response = Response::new(StatusCode::from_u16(written.status).unwrap_or(StatusCode::OK), body);
            for (name, value) in &written.headers {
                if let (Ok(name), Ok(value)) = (header::HeaderName::try_from(name), HeaderValue::try_from(value)) {
                    response.headers.append(name, value);
                }
            }
            out.insert(index, response);
        }
        out
    }

    /// The route handler answering `path`, unless a page pattern is more specific.
    fn handler(&self, path: &str) -> Option<(usize, Params)> {
        let (pattern, index, params) = self.handlers.find(path)?;
        if self.pages.find(path).is_some_and(|(page, _, _)| !pattern.ranks_before(page)) {
            return None;
        }
        Some((*index, params))
    }

    fn has_route(&self, route: MetadataRoute) -> bool {
        self.manifest.metadata_routes.iter().any(|name| name == route.name())
    }
}

/// Answer of the render methods; a streamed shell reports the same fields without `html`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rendered {
    #[serde(default)]
    html: String,
    status: u16,
    redirect: Option<RenderedRedirect>,
    /// Set when rendering fell back to another route, such as not-found.
    #[serde(default)]
    route: Option<String>,
    /// Pages Router props that replace the loader data in the serialized state.
    #[serde(default)]
    data: Option<Value>,
    /// The render ran out of time and left its pending Suspense boundaries to the browser.
    #[serde(default)]
    incomplete: bool,
    /// HTML returned by `useServerInsertedHTML` callbacks, for the head.
    #[serde(default)]
    head: String,
    #[serde(default)]
    not_found: Option<usize>,
    #[serde(default)]
    global_error: bool,
    #[serde(flatten)]
    hints: CacheHints,
}

/// React's output for [`Server::assemble`]: buffered, or the events that follow a streamed shell.
enum Output {
    Buffered(Vec<u8>),
    Streamed(tokio::sync::mpsc::UnboundedReceiver<StreamEvent>),
}

#[derive(Deserialize)]
struct RenderedRedirect {
    location: String,
    status: u16,
}

/// Caching hints from `getStaticProps`.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct CacheHints {
    #[serde(default)]
    revalidate: Option<u64>,
    #[serde(default, rename = "static")]
    is_static: bool,
}

/// Answer of `__VOLT__.props` for a Pages Router data request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PropsResult {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    not_found: bool,
    #[serde(default)]
    redirect: Option<RenderedRedirect>,
}

/// Answer of `__VOLT__.metadataRoute`: the default export and the module's segment config.
#[derive(Deserialize)]
struct MetadataOutput {
    value: Value,
    #[serde(default)]
    revalidate: Option<u64>,
    #[serde(default)]
    dynamic: bool,
}

/// A rendered page with the caching hints and props its data functions returned.
struct Page {
    response: Response,
    hints: CacheHints,
    data: Option<Value>,
    /// Rendered after a timeout with boundaries left to the browser: served, but never cached.
    incomplete: bool,
    /// The body when the page streams; `response.body` is empty then.
    stream: Option<PageStream>,
}

/// A response on its way out, with the streaming body of a page or a route handler when there is one.
struct Reply {
    response: Response,
    stream: Option<Either<PageStream, ScriptStream>>,
}

impl From<Response> for Reply {
    fn from(response: Response) -> Self {
        Self { response, stream: None }
    }
}

impl From<Page> for Reply {
    fn from(page: Page) -> Self {
        Self { response: page.response, stream: page.stream.map(Either::Left) }
    }
}

/// How React renders a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    /// `renderToString`, with `[server] streaming = false`.
    String,
    /// `renderToReadableStream` buffered until every Suspense boundary is ready: cached and prerendered
    /// pages, crawlers.
    Complete,
    /// `renderToReadableStream` sent as it is produced.
    Stream,
}

impl RenderMode {
    fn method(self) -> &'static str {
        match self {
            Self::String => "render",
            Self::Complete => "document",
            Self::Stream => "stream",
        }
    }
}

/// What `middleware.ts` leaves of a request that goes on.
enum Edge {
    /// Continue with the request headers it replaced (`ctx`) and the app path and query it rewrote to (`target`).
    Continue {
        ctx: Option<Ctx>,
        target: Option<String>,
    },
    Respond(Response),
}

/// How a page is requested: a document load, or a client navigation from the route the client shows.
#[derive(Clone, Copy)]
enum Navigation<'a> {
    Hard,
    Soft { from: Option<&'a str> },
}

/// Outcome of matching a path and running its loader.
enum Resolved {
    Page(Box<PageState>),
    /// Status and app path of a redirect raised by a loader.
    Redirect(StatusCode, String),
}

pub(crate) struct Server {
    config: Arc<Config>,
    state: Arc<State>,
    middleware: Vec<Middleware>,
    build: RwLock<Arc<Build>>,
    engine: Engine,
    actions: HashMap<&'static str, &'static ActionDef>,
    loaders: HashMap<String, &'static LoaderDef>,
    apis: HashMap<Method, Router<&'static ApiDef>>,
    cache: Arc<PageCache>,
    hub: Option<Hub>,
    /// The error of the last `volt dev` rebuild when it failed; the previous build keeps serving meanwhile.
    failure: RwLock<Option<String>>,
    /// User agents that get complete documents instead of streams.
    bots: Option<regex::Regex>,
    #[cfg(feature = "images")]
    images: crate::images::Images,
    /// Proxies middleware rewrites to other origins.
    fetcher: Fetcher,
    dev: bool,
}

impl Server {
    pub fn new(config: Config, state: State, middleware: Vec<Middleware>) -> Result<Arc<Self>> {
        let dev = crate::config::is_dev();
        let bots = config.server.bot_pattern().map_err(|e| Error::internal(e.0))?;
        let out_dir = config.out_dir();
        let build = Build::load(&out_dir)?;
        let engine = Engine::start(EngineOptions {
            bundle: out_dir.join(&build.manifest.server),
            workers: config.workers(),
            memory_limit: config.server.memory_limit_mb * 1024 * 1024,
            dev,
            fetch: crate::ssr::FetchOptions::from_config(&config.server),
        })?;

        let mut actions = HashMap::new();
        for action in registry::actions() {
            if actions.insert(action.name, action).is_some() {
                return Err(Error::internal(format!("duplicate action `{}`", action.name)));
            }
        }
        let loaders = registry::loaders().map(|l| (RoutePattern::parse(l.route).as_str().to_owned(), l)).collect();
        let mut apis: HashMap<Method, Router<&'static ApiDef>> = HashMap::new();
        for api in registry::apis() {
            let method = Method::from_bytes(api.method.as_bytes()).unwrap_or(Method::GET);
            apis.entry(method).or_default().insert(api.path, api);
        }

        #[cfg(feature = "images")]
        let images = crate::images::Images::new(
            config.public_dir(),
            config.base_path(),
            &out_dir,
            match config.images.concurrency {
                0 => config.workers(),
                n => n,
            },
        );
        let server = Arc::new(Self {
            #[cfg(feature = "images")]
            images,
            cache: PageCache::shared(config.server.page_cache_mb * 1024 * 1024, config.i18n.clone()),
            fetcher: Fetcher::new(FetchOptions::from_config(&config.server)),
            hub: dev.then(Hub::default),
            failure: RwLock::new(build.manifest.error.clone().filter(|_| dev)),
            bots,
            config: Arc::new(config),
            state: Arc::new(state),
            middleware,
            build: RwLock::new(Arc::new(build)),
            engine,
            actions,
            loaders,
            apis,
            dev,
        });
        if !dev {
            server.cache.preload(&server.current().manifest.routes, &out_dir, &server.config);
        }
        Ok(server)
    }

    fn current(&self) -> Arc<Build> {
        self.build.read().map(|b| b.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    /// Polls the manifest in development and hot-swaps the build when it changes.
    pub fn watch(self: &Arc<Self>) {
        let server = self.clone();
        let path = self.config.out_dir().join(crate::manifest::MANIFEST_FILE);
        std::thread::spawn(move || {
            let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let mut last: Option<SystemTime> = modified(&path);
            loop {
                std::thread::sleep(Duration::from_millis(150));
                let now = modified(&path);
                if now == last {
                    continue;
                }
                last = now;
                let out_dir = server.config.out_dir();
                match Build::load(&out_dir) {
                    Ok(build) => {
                        let (event, data) = crate::dev::event(&server.current().manifest, &build.manifest);
                        if let Ok(mut failure) = server.failure.write() {
                            failure.clone_from(&build.manifest.error);
                        }
                        // A failed rebuild leaves partial static files behind: the open pages keep loading
                        // modules from the previous build, which stays in memory.
                        if build.manifest.error.is_none() {
                            let bundle = out_dir.join(&build.manifest.server);
                            if let Ok(mut slot) = server.build.write() {
                                *slot = Arc::new(build);
                            }
                            server.engine.reload(bundle);
                        }
                        if let Some(hub) = &server.hub {
                            hub.broadcast(event, &data);
                        }
                    }
                    Err(error) => log::error(error.message()),
                }
            }
        });
    }

    pub async fn handle(self: Arc<Self>, request: hyper::Request<Incoming>, remote: SocketAddr) -> HyperResponse {
        let started = Instant::now();
        let (parts, body) = request.into_parts();
        let method = parts.method.clone();
        let path = parts.uri.path().to_owned();

        if self.dev
            && self.config.app_path(&path) == Some("/_volt/hmr")
            && let Some(hub) = &self.hub
        {
            return sse(hub.subscribe());
        }

        let body = match Limited::new(body, self.config.server.body_limit()).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return finish(Response::text("Payload Too Large").status(StatusCode::PAYLOAD_TOO_LARGE).into());
            }
        };
        let accept = parts.headers.clone();
        let request = RequestParts {
            method: parts.method,
            uri: parts.uri,
            headers: parts.headers,
            body,
            remote: Some(remote),
            started,
        };
        let ctx = Ctx::new(request, self.state.clone(), self.config.clone(), self.cache.clone());

        let mut reply = self.dispatch(&ctx).await;
        ctx.take_response().apply(&mut reply.response);
        self.localize_redirect(&mut reply.response);
        self.decorate(&mut reply, &accept);
        if self.dev && !path.starts_with(&self.config.static_prefix()) {
            log::request(method.as_str(), &path, reply.response.status.as_u16(), started.elapsed());
        }
        finish(reply)
    }

    async fn dispatch(self: &Arc<Self>, ctx: &Ctx) -> Reply {
        let Some(raw) = self.config.app_path(ctx.path()) else {
            return not_found_text().into();
        };
        if let Some(response) = self.normalize_slash(ctx, raw) {
            return response.into();
        }
        // Everything below, Rust and TypeScript middleware included, reads the one spelling the router resolves.
        let path = canonical_path(raw);
        let canonical;
        let ctx = if path == raw {
            ctx
        } else {
            let uri = self.config.public_path(&with_query(&path, ctx.uri().query())).parse().ok();
            canonical = ctx.with_request(uri, None);
            &canonical
        };
        if let Some(response) = self.redirect(&path) {
            return response.into();
        }
        let localized = ctx.with_locale(self.config.split_locale(&path).0);
        for middleware in &self.middleware {
            if let Some(response) = middleware(localized.clone()).await {
                return response.into();
            }
        }
        if let Some(asset) = path.strip_prefix(STATIC_PREFIX) {
            return self.current().assets.serve(asset, ctx.headers()).unwrap_or_else(not_found_text).into();
        }
        if let Some(name) = path.strip_prefix("/_volt/action/") {
            return self.action(ctx, name).await.into();
        }
        if path == "/_volt/data" {
            return self.data(ctx, &ctx.query_param("path").unwrap_or_else(|| "/".into())).await.into();
        }
        if let Some(stem) = path.strip_prefix("/_volt/data/").and_then(|p| p.strip_suffix(".json")) {
            let pathname = if stem == "index" { "/".to_owned() } else { format!("/{stem}") };
            let target = match ctx.uri().query() {
                Some(query) => format!("{pathname}?{query}"),
                None => pathname,
            };
            return self.data(ctx, &target).await.into();
        }
        #[cfg(feature = "images")]
        if path == "/_volt/image" && self.config.images.optimize {
            return self.images.serve(ctx.query_string()).await.into();
        }
        if let Some(error) = self.failure.read().ok().and_then(|f| f.clone()) {
            return Response::html(error_overlay("Build failed", &error, self.config.base_path()))
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .into();
        }
        // `spelled` is the path as requested, which the locale redirect compares with its canonical form.
        let (ctx, path, spelled) = match self.edge(ctx, &with_query(&path, ctx.uri().query())).await {
            Edge::Respond(response) => return response.into(),
            Edge::Continue { ctx: replaced, target } => {
                let ctx = replaced.unwrap_or_else(|| ctx.clone());
                match target {
                    Some(target) => {
                        let uri = self.config.public_path(&target).parse().ok();
                        let path = split_query(&target).0.to_owned();
                        (ctx.with_request(uri, None), path.clone(), path)
                    }
                    None => (ctx, path, raw.to_owned()),
                }
            }
        };
        let ctx = &ctx;
        let (prefix, route_path) = self.config.i18n.split(&path);
        let detects = self.detects_locale(ctx, prefix, route_path);
        if let Some(response) = self.locale_redirect(ctx, prefix, &spelled, route_path, detects) {
            return response.into();
        }
        let ctx = &ctx.with_locale(prefix.unwrap_or(self.config.default_locale()));
        let path = self.rewrite(route_path);
        // API routes, metadata files, public files and SEO files live outside the locale prefixes; below a prefix
        // they still answer when no page matches, so a localized link to `/api/x` or `/file.pdf` works.
        if prefix.is_none() || self.current().entry(&path).is_none() {
            if let Some(response) = self.api(ctx, &path).await {
                return response.into();
            }
            if let Some(reply) = self.route_handler(ctx, &path).await {
                return reply;
            }
            if matches!(*ctx.method(), Method::GET | Method::HEAD) {
                if let Some(response) = self.current().metadata.serve(&path, ctx.uri().query(), ctx.headers()) {
                    return response.into();
                }
                if let Some(response) = serve_public(&self.config.public_dir(), &path, ctx.headers()).await {
                    return response.into();
                }
                if let Some(response) = self.seo(ctx, &path).await {
                    return response.into();
                }
            }
        }
        let mut reply = self.page(ctx, &path).await;
        if detects {
            vary_on_locale(&mut reply.response);
        }
        reply
    }

    /// Whether this request is the unprefixed root that `[i18n] locale_detection` may redirect.
    fn detects_locale(&self, ctx: &Ctx, prefix: Option<&str>, route_path: &str) -> bool {
        let i18n = &self.config.i18n;
        i18n.is_enabled()
            && i18n.locale_detection
            && prefix.is_none()
            && route_path == "/"
            && matches!(*ctx.method(), Method::GET | Method::HEAD)
    }

    /// Redirects to the canonical locale prefix (none for the default locale, the configured spelling
    /// otherwise) and, on the root, to the visitor's preferred locale.
    fn locale_redirect(
        &self,
        ctx: &Ctx,
        prefix: Option<&str>,
        path: &str,
        route_path: &str,
        detects: bool,
    ) -> Option<Response> {
        let i18n = &self.config.i18n;
        if !i18n.is_enabled() || !matches!(*ctx.method(), Method::GET | Method::HEAD) {
            return None;
        }
        let query = ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
        if let Some(locale) = prefix {
            let canonical = i18n.localize(locale, route_path);
            return (canonical != path)
                .then(|| Response::redirect(StatusCode::PERMANENT_REDIRECT, &format!("{canonical}{query}")));
        }
        if !detects {
            return None;
        }
        let preferred = self
            .chosen_locale(ctx)
            .or_else(|| ctx.header("accept-language").and_then(|header| i18n.negotiate(header)))
            .filter(|locale| *locale != i18n.default_locale)?;
        let mut response = Response::redirect(StatusCode::TEMPORARY_REDIRECT, &format!("/{preferred}{query}"))
            .header(header::CACHE_CONTROL, HeaderValue::from_static(PRIVATE_CACHE));
        vary_on_locale(&mut response);
        Some(response)
    }

    /// Locale stored in the `VOLT_LOCALE` or `NEXT_LOCALE` cookie, when it is supported.
    fn chosen_locale(&self, ctx: &Ctx) -> Option<&str> {
        crate::config::LOCALE_COOKIES.iter().find_map(|name| self.config.i18n.find(&ctx.cookie(name)?))
    }

    /// Locale of the page that called an action: sent by the client runtime, else the referer's prefix.
    fn caller_locale(&self, ctx: &Ctx) -> String {
        let i18n = &self.config.i18n;
        let sent = ctx.header("x-volt-locale").and_then(|locale| i18n.find(locale));
        let referer = || {
            let uri: http::Uri = ctx.header("referer")?.parse().ok()?;
            i18n.split(self.config.app_path(uri.path())?).0
        };
        sent.or_else(referer).unwrap_or(self.config.default_locale()).to_owned()
    }

    /// Redirects with 308 to the canonical form of the URL under `trailing_slash`.
    fn normalize_slash(&self, ctx: &Ctx, path: &str) -> Option<Response> {
        if !matches!(*ctx.method(), Method::GET | Method::HEAD) || path.starts_with("/_volt/") {
            return None;
        }
        let canonical = self.config.canonical_path(ctx.path())?;
        let location = match ctx.uri().query() {
            Some(query) => format!("{canonical}?{query}"),
            None => canonical,
        };
        Some(Response::redirect(StatusCode::PERMANENT_REDIRECT, &location))
    }

    /// Redirects are produced with app paths; this resolves them against the base path once.
    fn localize_redirect(&self, response: &mut Response) {
        let Some(location) = response.headers.get(header::LOCATION).and_then(|v| v.to_str().ok()) else {
            return;
        };
        let public = self.config.public_path(location);
        if public != location
            && let Ok(value) = HeaderValue::from_str(&public)
        {
            response.headers.insert(header::LOCATION, value);
        }
    }

    /// Applies the first matching `[[redirects]]` rule. Rules match below every locale prefix
    /// (unless `locale = false`) and send a prefixed request to the same locale's destination.
    fn redirect(&self, path: &str) -> Option<Response> {
        let (prefix, route_path) = self.config.i18n.split(path);
        self.config.redirects.iter().find_map(|rule| {
            let localized = rule.locale && self.config.i18n.is_enabled();
            let params = RoutePattern::parse(&rule.source).matches(if localized { route_path } else { path })?;
            let mut destination = fill(&rule.destination, &params);
            if let Some(locale) = prefix.filter(|_| localized) {
                destination = self.localize_location(locale, &destination);
            }
            let status = if rule.permanent { StatusCode::PERMANENT_REDIRECT } else { StatusCode::TEMPORARY_REDIRECT };
            Some(Response::redirect(status, &destination))
        })
    }

    /// Redirect target `href` in `locale`. Targets served outside the locale prefixes (API routes,
    /// public files, SEO files) keep their path, so a German page can still redirect to `/api/logout`.
    fn localize_location(&self, locale: &str, href: &str) -> String {
        let path = &href[..href.find(['?', '#']).unwrap_or(href.len())];
        if href.starts_with('/') && self.unlocalized(&self.rewrite(path)) {
            return href.to_owned();
        }
        self.config.i18n.localize_href(locale, href)
    }

    /// Whether an app path names an API route, a route handler, a public file or an SEO file rather than a page.
    fn unlocalized(&self, path: &str) -> bool {
        self.apis.values().any(|router| router.find(path).is_some())
            || self.current().handler(path).is_some()
            || safe_join(&self.config.public_dir(), path).is_some_and(|file| file.is_file())
            || self.current().metadata.contains(path)
            || self.serves_seo(path)
    }

    /// Whether an SEO file is enabled in `[seo]` or built from a code metadata route.
    fn serves_seo(&self, path: &str) -> bool {
        let seo = &self.config.seo;
        let source = self.seo_source(path).is_some();
        match path {
            "/robots.txt" => seo.robots || source,
            "/sitemap.xml" => seo.sitemap || source,
            "/llms.txt" => seo.llms,
            "/manifest.webmanifest" => source,
            _ => false,
        }
    }

    fn rewrite(&self, path: &str) -> String {
        self.config
            .rewrites
            .iter()
            .find_map(|rule| RoutePattern::parse(&rule.source).matches(path).map(|p| fill(&rule.destination, &p)))
            .unwrap_or_else(|| path.to_owned())
    }

    async fn api(&self, ctx: &Ctx, path: &str) -> Option<Response> {
        let method = if *ctx.method() == Method::HEAD { Method::GET } else { ctx.method().clone() };
        if let Some((_, api, params)) = self.apis.get(&method).and_then(|router| router.find(path)) {
            return Some((api.handler)(ctx.with_params(params)).await);
        }
        let allowed = self.apis.iter().any(|(_, router)| router.find(path).is_some());
        allowed.then(|| Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED))
    }

    /// Runs the `route.ts` handler matching `path`, or serves its prerendered `GET` response.
    async fn route_handler(&self, ctx: &Ctx, path: &str) -> Option<Reply> {
        let build = self.current();
        let (index, params) = build.handler(path)?;
        if matches!(*ctx.method(), Method::GET | Method::HEAD)
            && let Some(response) = build.prerendered.get(&index)
        {
            return Some(response.clone().into());
        }
        let id = &build.manifest.handlers[index].id;
        let opened = match self.run_handler(ctx, id, path, &params, true) {
            Ok(events) => handlers::open(events).await,
            Err(error) => Err(error),
        };
        Some(match opened {
            Ok((response, stream)) => Reply { response, stream: stream.map(Either::Right) },
            Err(error) => {
                log::error(&format!("{} {path}: {}", ctx.method(), error.message()));
                self.script_error(&error).into()
            }
        })
    }

    /// Starts the handler of `route`; an `open` response may stream past the render deadline.
    fn run_handler(
        &self,
        ctx: &Ctx,
        route: &str,
        path: &str,
        params: &Params,
        open: bool,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>> {
        let target = ctx.uri().path_and_query().map_or("/", |p| p.as_str()).to_owned();
        let request =
            ScriptRequest { route: Some(route), params: params.to_json(), ..ScriptRequest::new(ctx, &target, path) };
        let (payload, body) = (request.to_json()?, ctx.body().clone());
        if open {
            self.engine.stream_response("handle", payload, body)
        } else {
            self.engine.stream_with("handle", payload, body)
        }
    }

    /// Renders a `force-static` route handler's `GET` response at build time.
    pub(crate) async fn prerender_handler(&self, id: &str) -> Result<Response> {
        collect(self.run_handler(&self.synthetic(id), id, id, &Params::default(), false)?).await
    }

    fn script_error(&self, error: &Error) -> Response {
        let message = if self.dev { error.message() } else { "Internal Server Error" };
        Response::text(message.to_owned()).status(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Runs `middleware.ts` for `target` (an app path and query) when its matcher covers the path. Response
    /// headers it adds go to the final response; a rewrite to another origin is proxied.
    async fn edge(&self, ctx: &Ctx, target: &str) -> Edge {
        let pass = Edge::Continue { ctx: None, target: None };
        let build = self.current();
        let Some(matcher) = build.middleware.as_ref().filter(|_| !target.starts_with("/_volt/")) else { return pass };
        let (path, query) = split_query(target);
        let lookup = |kind: ConditionKind, key: &str| match kind {
            ConditionKind::Header => ctx.header(key).map(str::to_owned),
            ConditionKind::Cookie => ctx.cookie(key),
            ConditionKind::Query => crate::form::parse_pairs(query.unwrap_or_default().as_bytes())
                .into_iter()
                .find(|(name, _)| name == key)
                .and_then(|(_, values)| values.into_iter().next()),
            ConditionKind::Host => ctx.header("host").map(hostname),
        };
        let matchers = match matcher.check(path, &lookup) {
            Verdict::No => return pass,
            Verdict::Yes => Vec::new(),
            Verdict::Ask(patterns) => patterns,
        };
        let public = self.config.public_path(target);
        let request = ScriptRequest { matchers, ..ScriptRequest::new(ctx, &public, path) };
        let response =
            match request.to_json().and_then(|json| self.engine.stream_with("middleware", json, ctx.body().clone())) {
                Ok(events) => collect(events).await,
                Err(error) => Err(error),
            };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                log::error(&format!("middleware {path}: {}", error.message()));
                return Edge::Respond(self.script_error(&error));
            }
        };
        let (rewrite, headers, request) = match Outcome::read(response) {
            Outcome::Respond(response) => return Edge::Respond(response),
            Outcome::Continue { rewrite, headers, request } => (rewrite, headers, request),
        };
        for (name, value) in &headers {
            if let Ok(value) = value.to_str() {
                ctx.set_header(name.as_str(), value);
            }
        }
        let replaced = request.map(|headers| ctx.with_request(None, Some(headers)));
        let Some(url) = rewrite else { return Edge::Continue { ctx: replaced, target: None } };
        match self.local_target(ctx, &url) {
            Some(target) => Edge::Continue { ctx: replaced, target: Some(target) },
            None => Edge::Respond(self.proxy(replaced.as_ref().unwrap_or(ctx), &url).await),
        }
    }

    /// App path and query of a same-origin URL, `None` for another origin.
    fn local_target(&self, ctx: &Ctx, url: &str) -> Option<String> {
        let public = match url.strip_prefix(&origin(ctx)) {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '?']) => rest,
            _ if url.starts_with('/') && !url.starts_with("//") => url,
            _ => return None,
        };
        let (path, query) = split_query(public);
        let app = self.config.app_path(if path.is_empty() { "/" } else { path })?;
        Some(with_query(app, query))
    }

    /// Answers with the response of another origin, as `NextResponse.rewrite` to an external URL does.
    async fn proxy(&self, ctx: &Ctx, url: &str) -> Response {
        let headers = ctx
            .headers()
            .iter()
            .filter(|(name, _)| *name != header::HOST && *name != header::CONNECTION)
            .filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
            .collect();
        let request =
            FetchRequest { url: url.to_owned(), method: ctx.method().to_string(), headers, redirect: "manual".into() };
        match self.fetcher.send(request, ctx.body().clone()).await {
            Ok((head, body)) => {
                let mut response =
                    Response::new(StatusCode::from_u16(head.status).unwrap_or(StatusCode::BAD_GATEWAY), body);
                for (name, value) in head.headers {
                    let framing = ["content-length", "transfer-encoding", "connection"].contains(&name.as_str());
                    if let (false, Ok(name), Ok(value)) =
                        (framing, header::HeaderName::try_from(name), HeaderValue::try_from(value))
                    {
                        response.headers.append(name, value);
                    }
                }
                response
            }
            Err(message) => {
                log::error(&format!("rewrite to {url}: {message}"));
                Response::text("Bad Gateway").status(StatusCode::BAD_GATEWAY)
            }
        }
    }

    /// A data request answered by middleware itself: redirects become client navigations, anything else
    /// makes the client load the document.
    fn edge_data(&self, ctx: &Ctx, response: Response) -> Response {
        let location = response.headers.get(header::LOCATION).and_then(|v| v.to_str().ok());
        let body = match location.filter(|_| response.status.is_redirection()) {
            Some(location) => {
                json!({ "redirect": self.local_target(ctx, location).unwrap_or_else(|| location.to_owned()) })
            }
            None => json!({ "document": true }),
        };
        let mut answer = Response::json(&body).header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        for cookie in response.headers.get_all(header::SET_COOKIE) {
            answer.headers.append(header::SET_COOKIE, cookie.clone());
        }
        answer
    }

    async fn action(&self, ctx: &Ctx, name: &str) -> Response {
        if *ctx.method() != Method::POST {
            return Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED);
        }
        let ctx = &ctx.with_locale(&self.caller_locale(ctx));
        let content_type = ctx.header("content-type").unwrap_or_default();
        match FormData::from_request(content_type, ctx.body()) {
            Ok(form) => self.run_action(ctx, name, form, None).await,
            Err(error) => Error::bad_request(error.to_string()).into_response(),
        }
    }

    /// Runs an action for a JavaScript call, a form posted to its action URL, or a form posted to the
    /// page URL given as `useActionState` permalink (`page`).
    async fn run_action(&self, ctx: &Ctx, name: &str, form: Option<FormData>, page: Option<&str>) -> Response {
        if !same_origin(ctx) {
            return Error::forbidden("cross-origin action call rejected").into_response();
        }
        let Some(action) = self.actions.get(name) else {
            return Error::new(StatusCode::NOT_FOUND, format!("unknown action `{name}`")).into_response();
        };
        let native = form.as_ref().filter(|_| ctx.header("x-volt-action").is_none()).map(|form| NativeForm {
            back: return_path(ctx, page.or_else(|| form.text("redirect"))),
            stateful: form.marker().stateful,
        });
        let payload = match form {
            Some(form) => Payload::Form(form),
            None => Payload::Json(ctx.body().clone()),
        };
        let result = (action.handler)(ctx.clone(), payload).await;
        // Cookies usually change what loaders return (sessions), so the client's route data is stale.
        // The server cache is shared between users and stays: one user's cookie cannot stale it.
        if ctx.sets_cookies() {
            ctx.refresh_client();
        }
        if let Some(native) = native {
            return self.form_result(ctx, name, native, result);
        }
        match result {
            Ok(data) => Response::json(&json!({ "data": data })),
            Err(error) => {
                if error.status().is_server_error() {
                    log::error(&format!("action `{name}` failed: {}", error.message()));
                }
                Response::json(&json!({ "error": error.to_json(self.dev) })).status(match error.kind() {
                    ErrorKind::Redirect(_) => StatusCode::OK,
                    _ => error.status(),
                })
            }
        }
    }

    /// Answers a no-JavaScript form submission with a redirect back to the page, carrying the result
    /// for `useActionState`. Errors of forms without it render a plain error page.
    fn form_result(&self, ctx: &Ctx, name: &str, form: NativeForm, result: Result<Value>) -> Response {
        let error = match result {
            Ok(data) => {
                ActionFlash::data(ctx, name, &data);
                return Response::redirect(StatusCode::SEE_OTHER, &form.back);
            }
            Err(error) => error,
        };
        if let ErrorKind::Redirect(location) = error.kind() {
            return Response::redirect(StatusCode::SEE_OTHER, &self.localize_location(ctx.locale(), location));
        }
        if error.status().is_server_error() {
            log::error(&format!("action `{name}` failed: {}", error.message()));
        }
        if form.stateful && ActionFlash::error(ctx, name, &error.to_json(self.dev)) {
            return Response::redirect(StatusCode::SEE_OTHER, &form.back);
        }
        use crate::document::escape_html;
        let message = escape_html(error.client_message(self.dev));
        let fields: String = error
            .field_errors()
            .into_iter()
            .flatten()
            .flat_map(|(field, messages)| messages.iter().map(move |m| (field, m)))
            .map(|(field, m)| format!("<li><b>{}</b>: {}</li>", escape_html(field), escape_html(m)))
            .collect();
        let fields = if fields.is_empty() { fields } else { format!("<ul>{fields}</ul>") };
        Response::html(format!(
            "<!DOCTYPE html><meta charset=\"utf-8\"><title>Error</title><p>{message}</p>{fields}<p><a href=\"{}\">Back</a></p>",
            escape_html(&form.back)
        ))
        .status(error.status())
    }

    async fn data(self: &Arc<Self>, ctx: &Ctx, target: &str) -> Response {
        let (pathname, query) = split_query(target);
        if !pathname.starts_with('/') {
            return Error::bad_request("the data path must start with `/`").into_response();
        }
        let target = &with_query(&canonical_path(pathname), query);
        let (ctx, target) = match self.edge(ctx, target).await {
            Edge::Respond(response) => return self.edge_data(ctx, response),
            Edge::Continue { ctx: replaced, target: rewritten } => {
                (replaced.unwrap_or_else(|| ctx.clone()), rewritten.unwrap_or_else(|| target.to_owned()))
            }
        };
        let (ctx, target) = (&ctx, target.as_str());
        let (pathname, search) = match target.split_once('?') {
            Some((p, s)) => (p.to_owned(), format!("?{s}")),
            None => (target.to_owned(), String::new()),
        };
        let (locale, route_path) = self.config.split_locale(&pathname);
        let ctx = &ctx.with_locale(locale);
        let pathname = self.rewrite(route_path);
        let navigation = Navigation::Soft { from: ctx.header(FROM_HEADER) };
        let resolved = match self.resolve(ctx, &pathname, &search, navigation).await {
            Ok(state) if state.retain => Ok(state),
            Ok(state) => match self.static_props(ctx, &state).await {
                Some(data) => Ok(PageState { data, ..state }),
                None => self.with_props(state).await,
            },
            Err(error) => Err(error),
        };
        match self.settle(ctx, resolved, &pathname, &search) {
            Resolved::Page(state) => {
                Response::json(&state).header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
            }
            Resolved::Redirect(_, location) => Response::json(&json!({ "redirect": location })),
        }
    }

    /// Props of a `getStaticProps` page from the page cache, rendering the page first on a miss, so that
    /// navigations show the same data as the cached HTML.
    async fn static_props(self: &Arc<Self>, ctx: &Ctx, state: &PageState) -> Option<Value> {
        let build = self.current();
        if self.dev || !build.manifest.route(&state.route).is_some_and(|r| self.is_static_route(r)) {
            return None;
        }
        let page = self.serve_page(ctx, &state.pathname).await;
        if page.response.status == StatusCode::OK { page.data } else { None }
    }

    /// Whether a route renders the same HTML for everyone: a `getStaticProps` page without a Rust loader.
    fn is_static_route(&self, route: &RouteEntry) -> bool {
        route.static_props && !self.loaders.contains_key(&route.id)
    }

    /// Runs the Pages Router data functions of the resolved route for a client navigation.
    async fn with_props(&self, mut state: PageState) -> Result<PageState> {
        if !self.current().manifest.route(&state.route).is_some_and(|r| r.pages) {
            return Ok(state);
        }
        let output = self.engine.call("props", serde_json::to_string(&state)?).await?;
        let result: PropsResult = serde_json::from_str(&output)?;
        if result.not_found {
            return Err(Error::not_found());
        }
        if let Some(redirect) = result.redirect {
            return Err(Error::redirect(redirect.location));
        }
        state.data = result.data.unwrap_or(Value::Null);
        Ok(state)
    }

    /// `robots.txt`, `sitemap.xml`, `llms.txt` and `manifest.webmanifest`. Output built from a code metadata
    /// route is rendered once and shared, as in Next.js, until its `revalidate` time or a revalidation.
    async fn seo(&self, ctx: &Ctx, path: &str) -> Option<Response> {
        let key = cache::key(path, "", ctx.locale());
        let shared = !self.dev && self.seo_source(path).is_some();
        if shared {
            match self.cache.get(&key) {
                Lookup::Fresh(page) | Lookup::Stale { page, refresh: false } => return Some(page.response),
                Lookup::Stale { .. } | Lookup::Miss => {}
            }
        }
        let ticket = self.cache.ticket();
        let (mut response, policy) = match self.render_seo(ctx, path).await? {
            Ok(rendered) => rendered,
            Err(error) => {
                log::error(&format!("{path}: {}", error.message()));
                (Response::text("Internal Server Error").status(StatusCode::INTERNAL_SERVER_ERROR), None)
            }
        };
        match policy {
            Some(revalidate) if response.status == StatusCode::OK => {
                response.headers.insert(header::CACHE_CONTROL, cache::cache_control(revalidate));
                let page = CachedPage { response: response.clone(), data: None };
                self.cache.put(key, &page, revalidate.map(Duration::from_secs), ticket);
            }
            _ if shared => self.cache.release(&key),
            _ => {}
        }
        Some(response)
    }

    /// The code metadata route an SEO file is built from: `sitemap.ts` also feeds `llms.txt`.
    fn seo_source(&self, path: &str) -> Option<MetadataRoute> {
        let route = match path {
            "/llms.txt" if self.config.seo.llms => MetadataRoute::Sitemap,
            _ => MetadataRoute::from_path(path)?,
        };
        self.current().has_route(route).then_some(route)
    }

    /// Renders an SEO file, with `Some(revalidate)` when the output may be shared with every visitor.
    async fn render_seo(&self, ctx: &Ctx, path: &str) -> Option<Result<(Response, Option<Option<u64>>)>> {
        if !self.serves_seo(path) {
            return None;
        }
        let source = self.seo_source(path);
        let output = match source {
            Some(route) => match self.metadata_route(route).await {
                Ok(output) => Some(output),
                Err(error) => return Some(Err(error)),
            },
            None => None,
        };
        let policy = output.as_ref().filter(|o| !o.dynamic && !self.dev).map(|o| o.revalidate);
        // A shared file must not depend on the visitor, so `#[sitemap]` providers then see a bare request.
        let synthetic;
        let ctx = match policy {
            Some(_) => {
                synthetic = self.synthetic(path);
                &synthetic
            }
            None => ctx,
        };
        let value = output.map(|o| o.value);
        let response = match path {
            "/robots.txt" => match value {
                Some(value) => serde_json::from_value::<Robots>(value)
                    .map(|robots| Response::text(robots.render()))
                    .map_err(Error::from),
                None => Ok(Response::text(seo::robots(&self.config))),
            },
            "/manifest.webmanifest" => Ok(Response::with_type(
                StatusCode::OK,
                "application/manifest+json",
                value.unwrap_or_default().to_string(),
            )),
            "/sitemap.xml" => self.sitemap_entries(ctx, value).await.map(|entries| {
                Response::with_type(StatusCode::OK, "application/xml", seo::sitemap(&self.config, &entries))
            }),
            _ => {
                self.sitemap_entries(ctx, value).await.map(|entries| Response::text(seo::llms(&self.config, &entries)))
            }
        };
        Some(response.map(|response| (response, policy)))
    }

    /// Evaluates the default export of `app/sitemap.ts`, `app/robots.ts` or `app/manifest.ts`.
    async fn metadata_route(&self, route: MetadataRoute) -> Result<MetadataOutput> {
        let output = self.engine.call("metadataRoute", route.name().to_owned()).await?;
        Ok(serde_json::from_str(&output)?)
    }

    /// Sitemap URLs: the output of `app/sitemap.ts` when present, else every known path, followed by the
    /// `#[sitemap]` providers.
    async fn sitemap_entries(&self, ctx: &Ctx, code: Option<Value>) -> Result<Vec<SitemapEntry>> {
        let build = self.current();
        let mut entries: Vec<SitemapEntry> = Vec::new();
        if let Some(code) = code {
            entries = serde_json::from_value(code)?;
        } else {
            for route in build.manifest.routes.iter().filter(|r| r.is_page()) {
                if !route.paths.is_empty() {
                    entries.extend(route.paths.iter().map(|p| SitemapEntry::new(p.clone())));
                } else if !RoutePattern::parse(&route.id).is_dynamic() {
                    entries.push(SitemapEntry::new(route.id.clone()));
                }
            }
        }
        for provider in registry::sitemaps() {
            match (provider.handler)(ctx.clone()).await {
                Ok(more) => entries.extend(more),
                Err(error) => log::error(&format!("sitemap provider failed: {}", error.message())),
            }
        }
        Ok(entries)
    }

    async fn page(self: &Arc<Self>, ctx: &Ctx, path: &str) -> Reply {
        if *ctx.method() == Method::POST
            && let Some(form) = permalink_form(ctx)
            && let Some(name) = form.marker().action.clone()
        {
            let page = ctx.config().app_path(ctx.uri().path()).unwrap_or(path);
            let page = match ctx.uri().query() {
                Some(query) => format!("{page}?{query}"),
                None => page.to_owned(),
            };
            return self.run_action(ctx, &name, Some(form), Some(&page)).await.into();
        }
        if matches!(*ctx.method(), Method::GET | Method::HEAD)
            && let Some(flash) = ActionFlash::take(ctx)
        {
            return self.flash_page(ctx, path, flash).await;
        }
        self.serve_page(ctx, path).await.into()
    }

    /// Renders a page for one visitor with the result of their no-JavaScript action submission, bypassing the cache.
    async fn flash_page(&self, ctx: &Ctx, path: &str, flash: Value) -> Reply {
        let build = self.current();
        let search = ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
        let mut page = match self.resolve_page(ctx, path, &search).await {
            Resolved::Page(mut state) => {
                state.action = Some(flash);
                self.render(&build, *state, self.mode(Some(ctx), false)).await
            }
            Resolved::Redirect(status, location) => return Response::redirect(status, &location).into(),
        };
        page.response.headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(PRIVATE_CACHE));
        page.into()
    }

    /// Streams unless the page is shared (cached, prerendered), has no visitor, the visitor is a crawler,
    /// or streaming is off.
    fn mode(&self, ctx: Option<&Ctx>, shared: bool) -> RenderMode {
        if !self.config.server.streaming {
            return RenderMode::String;
        }
        let Some(ctx) = ctx.filter(|_| !shared) else {
            return RenderMode::Complete;
        };
        let agent = ctx.header("user-agent").unwrap_or_default();
        if self.bots.as_ref().is_some_and(|bots| bots.is_match(agent)) {
            RenderMode::Complete
        } else {
            RenderMode::Stream
        }
    }

    async fn serve_page(self: &Arc<Self>, ctx: &Ctx, path: &str) -> Page {
        if !matches!(*ctx.method(), Method::GET | Method::HEAD) {
            return Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED).into();
        }
        let build = self.current();
        let entry = build.entry(path);
        let is_static = entry.is_some_and(|e| self.is_static_route(e));
        // `getStaticProps` and `force-static` pages cannot see the query, so one entry per pathname serves
        // every query string.
        let (query, search) = if is_static || entry.is_some_and(|e| e.force_static) {
            ("", String::new())
        } else {
            (ctx.query_string(), ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default())
        };
        let cacheable = !self.dev && entry.is_some_and(|e| e.is_cached() || is_static);
        if !cacheable {
            return self.generate(ctx, &build, path, &search, self.mode(Some(ctx), false)).await;
        }
        let key = cache::key(path, query, ctx.locale());
        match self.cache.get(&key) {
            Lookup::Fresh(page) => return page.into(),
            Lookup::Stale { page, refresh } => {
                if refresh {
                    self.refresh(key, path.to_owned(), search, ctx.locale().to_owned());
                }
                return page.into();
            }
            Lookup::Miss => {}
        }
        let ticket = self.cache.ticket();
        let page = self.generate(ctx, &build, path, &search, self.mode(Some(ctx), true)).await;
        if page.response.status == StatusCode::OK && !page.incomplete {
            self.cache.put(key, &page.cached(), Self::ttl(&build, path, &page), ticket);
        }
        page
    }

    /// Cache lifetime of a rendered page: `revalidate` from `getStaticProps`, else from the route.
    fn ttl(build: &Build, path: &str, page: &Page) -> Option<Duration> {
        page.hints.revalidate.or_else(|| build.entry(path).and_then(|e| e.revalidate)).map(Duration::from_secs)
    }

    /// Regenerates a stale page in the background while the stale copy keeps being served.
    fn refresh(self: &Arc<Self>, key: String, path: String, search: String, locale: String) {
        let server = self.clone();
        tokio::spawn(async move {
            let ctx = server.synthetic(&format!("{}{search}", server.config.i18n.localize(&locale, &path)));
            let build = server.current();
            let ticket = server.cache.ticket();
            let page = server.generate(&ctx, &build, &path, &search, server.mode(None, true)).await;
            match page.response.status {
                StatusCode::OK if !page.incomplete => {
                    server.cache.put(key, &page.cached(), Self::ttl(&build, &path, &page), ticket)
                }
                StatusCode::NOT_FOUND => server.cache.remove(&key),
                status => {
                    log::warn(&format!("revalidating {path} returned {status}; serving the stale page"));
                    server.cache.release(&key);
                }
            }
        });
    }

    /// Resolves and renders a page without consulting the cache.
    async fn generate(&self, ctx: &Ctx, build: &Build, path: &str, search: &str, mode: RenderMode) -> Page {
        match self.resolve_page(ctx, path, search).await {
            Resolved::Page(state) => self.render(build, *state, mode).await,
            Resolved::Redirect(status, location) => Response::redirect(status, &location).into(),
        }
    }

    /// A GET request without headers for a localized app path, used for prerendering and background regeneration.
    fn synthetic(&self, app_path: &str) -> Ctx {
        let pathname = app_path.split_once('?').map_or(app_path, |(path, _)| path);
        let locale = self.config.split_locale(pathname).0.to_owned();
        let request = RequestParts {
            method: Method::GET,
            uri: self.config.public_path(app_path).parse().unwrap_or_default(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
            remote: None,
            started: Instant::now(),
        };
        Ctx::new(request, self.state.clone(), self.config.clone(), self.cache.clone()).with_locale(&locale)
    }

    /// Renders one concrete path (with its locale prefix) ahead of time, with the route data the client navigates with.
    pub(crate) async fn prerender(&self, path: &str) -> Prerendered {
        let ctx = self.synthetic(path);
        let build = self.current();
        match self.resolve_page(&ctx, self.config.split_locale(path).1, "").await {
            Resolved::Page(mut state) => {
                let page = self.render(&build, (*state).clone(), self.mode(None, true)).await;
                if let Some(data) = page.data {
                    state.data = data;
                }
                // A page that timed out at build time is left to be rendered on request.
                let status = if page.incomplete { StatusCode::SERVICE_UNAVAILABLE } else { page.response.status };
                Prerendered {
                    path: path.to_owned(),
                    status,
                    html: String::from_utf8_lossy(&page.response.body).into_owned(),
                    data: serde_json::to_string(&state).unwrap_or_default(),
                    revalidate: page.hints.revalidate,
                }
            }
            Resolved::Redirect(status, location) => Prerendered {
                path: path.to_owned(),
                status,
                html: String::new(),
                data: json!({ "redirect": location }).to_string(),
                revalidate: None,
            },
        }
    }

    /// Renders the not-found page, written as `404.html` by `volt export`.
    pub(crate) async fn prerender_not_found(&self) -> Prerendered {
        let path = "/404";
        let state = self.failure_state(path, "", &Error::not_found());
        let data = serde_json::to_string(&state).unwrap_or_default();
        let page = self.render(&self.current(), state, self.mode(None, true)).await;
        let html = String::from_utf8_lossy(&page.response.body).into_owned();
        Prerendered { path: path.to_owned(), status: page.response.status, html, data, revalidate: None }
    }

    /// `robots.txt`, `sitemap.xml`, `llms.txt` and `manifest.webmanifest`, as enabled in `[seo]` or defined in `app/`.
    pub(crate) async fn generated_files(&self) -> Vec<(String, Bytes)> {
        let ctx = self.synthetic("/");
        let mut files = Vec::new();
        for name in ["/robots.txt", "/sitemap.xml", "/llms.txt", "/manifest.webmanifest"] {
            if let Some(response) = self.seo(&ctx, name).await.filter(|r| r.status == StatusCode::OK) {
                files.push((name.trim_start_matches('/').to_owned(), response.body));
            }
        }
        files
    }

    async fn resolve_page(&self, ctx: &Ctx, path: &str, search: &str) -> Resolved {
        let resolved = self.resolve(ctx, path, search, Navigation::Hard).await;
        self.settle(ctx, resolved, path, search)
    }

    /// Turns a loader error into a redirect or the error page state, in the request's locale.
    fn settle(&self, ctx: &Ctx, resolved: Result<PageState>, path: &str, search: &str) -> Resolved {
        let mut state = match resolved {
            Ok(state) => state,
            Err(error) => match error.kind() {
                ErrorKind::Redirect(location) => {
                    return Resolved::Redirect(error.status(), self.localize_location(ctx.locale(), location));
                }
                _ => self.failure_state(path, search, &error),
            },
        };
        state.locale = ctx.locale().to_owned();
        Resolved::Page(Box::new(state))
    }

    /// Matches the route, runs its loader and resolves its parallel route slots.
    ///
    /// On a client navigation from a route that shares a layout with an intercepting route, the intercepting route
    /// wins: in a slot, the client keeps its page and only the slots change; otherwise it renders instead of the target.
    /// A URL that no page matches but a slot of a mounted layout does keeps the page as well; otherwise the `default`
    /// page of the layout renders, if there is one.
    async fn resolve(&self, ctx: &Ctx, pathname: &str, search: &str, navigation: Navigation<'_>) -> Result<PageState> {
        let build = self.current();
        let from = match navigation {
            Navigation::Soft { from: Some(id) } => build.manifest.route(id).filter(|r| !r.segments.is_empty()),
            _ => None,
        };
        if let Some(from) = from
            && build.slots.intercepts(&from.segments, pathname)
        {
            return self.retain(ctx, &build, from, pathname, search).await;
        }
        let in_scope = |index: &usize| {
            let scope = build.manifest.routes[*index].intercept.as_ref().map(|i| &i.scope);
            from.zip(scope).is_some_and(|(from, scope)| from.segments.contains(scope))
        };
        let page = build.intercepts.find_by(pathname, in_scope).or_else(|| build.pages.find(pathname));
        if page.is_none()
            && let Some(from) = from
            && build.slots.matches(&from.segments, pathname)
        {
            return self.retain(ctx, &build, from, pathname, search).await;
        }
        let Some((_, index, params)) = page.or_else(|| build.fallbacks.find(pathname)) else {
            return Err(Error::not_found());
        };
        let entry = &build.manifest.routes[*index];
        if !self.dev && !entry.accepts(pathname) {
            return Err(Error::not_found());
        }
        // `force-static` pages render once per pathname and are shared with every visitor, so their loader
        // sees a request without query, cookies or headers, as in Next.js.
        let search = if entry.force_static { "" } else { search };
        let synthetic;
        let ctx = if entry.force_static {
            synthetic = self.synthetic(pathname);
            &synthetic
        } else {
            ctx
        };
        let data = self.load(ctx, &entry.id, &params).await?;
        let mounted = from.map_or(&[][..], |from| &from.segments[..]);
        let slots = self.slots(ctx, &build, &entry.segments, pathname, false, mounted).await?;
        let mut state = self.state(entry.id.clone(), pathname, search, params.to_json(), data, 200, None);
        state.slots = slots;
        Ok(state)
    }

    /// Runs the loader registered for a page or slot page, if any.
    async fn load(&self, ctx: &Ctx, route: &str, params: &Params) -> Result<Value> {
        match self.loaders.get(route) {
            Some(loader) => (loader.handler)(ctx.with_params(params.clone())).await,
            None => Ok(Value::Null),
        }
    }

    /// What every slot of the layouts in `dirs` renders. `mounted` lists the layouts the client keeps from the
    /// view it navigates from: their unmatched slots keep what they showed. Any other slot without a match or a
    /// `default` page makes the whole page a 404, as in Next.js.
    async fn slots(
        &self,
        ctx: &Ctx,
        build: &Build,
        dirs: &[String],
        pathname: &str,
        intercept: bool,
        mounted: &[String],
    ) -> Result<BTreeMap<String, SlotState>> {
        let mut out = BTreeMap::new();
        for slot in build.slots.resolve(dirs, pathname, intercept) {
            let state = match slot.route {
                Some((route, params)) => {
                    let (data, error) = match self.load(ctx, &route.id, &params).await {
                        Ok(data) => (data, None),
                        Err(error) => (Value::Null, Some(self.slot_error(route, pathname, error)?)),
                    };
                    SlotState { route: Some(route.id.clone()), data, params: params.to_json(), error }
                }
                None if !slot.default && !mounted.iter().any(|dir| dir == slot.segment) => {
                    return Err(Error::not_found());
                }
                None => SlotState::default(),
            };
            out.insert(slot.key.to_owned(), state);
        }
        Ok(out)
    }

    /// A slot loader failure that a `not-found` or `error` boundary inside the slot renders, or the error itself
    /// when the slot has none and the whole page fails.
    fn slot_error(&self, route: &SlotRoute, pathname: &str, error: Error) -> Result<Value> {
        let caught = match error.kind() {
            ErrorKind::NotFound => route.not_found,
            ErrorKind::Redirect(_) => false,
            ErrorKind::Status => route.error,
        };
        if !caught {
            return Err(error);
        }
        if error.status().is_server_error() {
            log::error(&format!("{pathname} ({}): {}", route.id, error.message()));
        }
        Ok(error.to_json(self.dev))
    }

    /// A client navigation that keeps the page of `from` and only updates the slots of its layouts.
    async fn retain(
        &self,
        ctx: &Ctx,
        build: &Build,
        from: &RouteEntry,
        pathname: &str,
        search: &str,
    ) -> Result<PageState> {
        let slots = self.slots(ctx, build, &from.segments, pathname, true, &from.segments).await?;
        let mut state = self.state(from.id.clone(), pathname, search, json!({}), Value::Null, 200, None);
        state.slots = slots;
        state.retain = true;
        Ok(state)
    }

    fn failure_state(&self, pathname: &str, search: &str, error: &Error) -> PageState {
        if *error.kind() == ErrorKind::NotFound {
            return self.state(NOT_FOUND_ROUTE.into(), pathname, search, json!({}), Value::Null, 404, None);
        }
        if error.status().is_server_error() {
            log::error(&format!("{pathname}: {}", error.message()));
        }
        let detail = error.to_json(self.dev);
        self.state(ERROR_ROUTE.into(), pathname, search, json!({}), Value::Null, error.status().as_u16(), Some(detail))
    }

    #[allow(clippy::too_many_arguments)]
    fn state(
        &self,
        route: String,
        pathname: &str,
        search: &str,
        params: Value,
        data: Value,
        status: u16,
        error: Option<Value>,
    ) -> PageState {
        PageState {
            route,
            pathname: pathname.to_owned(),
            search: search.to_owned(),
            params,
            search_params: search_params(search),
            data,
            status,
            error,
            action: None,
            not_found: None,
            global_error: false,
            build_id: self.current().manifest.build_id.clone(),
            site: Site { name: self.config.site.name.clone(), url: self.config.site_url().to_owned() },
            locale: self.config.default_locale().to_owned(),
            export: crate::config::is_export(),
            slots: BTreeMap::new(),
            retain: false,
        }
    }

    async fn render(&self, build: &Build, state: PageState, mode: RenderMode) -> Page {
        match self.render_once(build, &state, mode).await {
            Ok(page) => page,
            Err(error) if self.dev => {
                log::error(error.message());
                Response::html(error_overlay("Render failed", error.message(), self.config.base_path()))
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .into()
            }
            Err(error) => {
                log::error(&format!("render {} failed: {}", state.pathname, error.message()));
                let fallback = PageState {
                    route: ERROR_ROUTE.into(),
                    status: 500,
                    error: Some(Error::internal("render failed").to_json(false)),
                    ..state
                };
                let mode = if mode == RenderMode::Stream { RenderMode::Complete } else { mode };
                match self.render_once(build, &fallback, mode).await {
                    Ok(page) => page,
                    Err(_) => Response::html("<!DOCTYPE html><title>500</title><h1>500 Internal Server Error</h1>")
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .into(),
                }
            }
        }
    }

    async fn render_once(&self, build: &Build, state: &PageState, mode: RenderMode) -> Result<Page> {
        let payload = serde_json::to_string(state)?;
        if mode == RenderMode::String {
            let output = self.engine.call(mode.method(), payload).await?;
            return self.buffered(build, state, &output);
        }
        let mut events = self.engine.stream(mode.method(), payload)?;
        let shell = match events.recv().await {
            Some(StreamEvent::Shell(shell)) => serde_json::from_str(&shell)?,
            // Pages Router routes and redirects answer with a complete result instead of a shell.
            Some(StreamEvent::Done(Ok(output))) => return self.buffered(build, state, &output),
            Some(StreamEvent::Done(Err(message))) => return Err(Error::internal(message)),
            Some(StreamEvent::Chunk(_)) | None => {
                return Err(Error::internal("the render stream ended before its shell"));
            }
        };
        if mode == RenderMode::Stream {
            return self.assemble(build, state, shell, Output::Streamed(events));
        }
        // The chunks are collected here rather than joined into a JavaScript string and parsed back.
        let mut html = Vec::new();
        loop {
            match events.recv().await {
                Some(StreamEvent::Chunk(chunk)) => html.extend_from_slice(&chunk),
                Some(StreamEvent::Done(Ok(_))) => break,
                Some(StreamEvent::Done(Err(message))) => return Err(Error::internal(message)),
                Some(StreamEvent::Shell(_)) | None => return Err(Error::internal("the render stream ended early")),
            }
        }
        self.assemble(build, state, shell, Output::Buffered(html))
    }

    /// Assembles a complete JSON result (`{ html, status, ... }`).
    fn buffered(&self, build: &Build, state: &PageState, output: &str) -> Result<Page> {
        let mut rendered: Rendered = serde_json::from_str(output)?;
        let html = std::mem::take(&mut rendered.html).into_bytes();
        self.assemble(build, state, rendered, Output::Buffered(html))
    }

    /// Builds the page response around React's output, or around the stream that follows its shell.
    fn assemble(&self, build: &Build, state: &PageState, rendered: Rendered, output: Output) -> Result<Page> {
        if let Some(redirect) = rendered.redirect {
            let status = StatusCode::from_u16(redirect.status).unwrap_or(StatusCode::TEMPORARY_REDIRECT);
            let location = self.localize_location(&state.locale, &redirect.location);
            return Ok(Response::redirect(status, &location).into());
        }
        let mut state = state.clone();
        state.status = rendered.status;
        if let Some(route) = rendered.route {
            state.route = route;
        }
        if let Some(data) = &rendered.data {
            state.data = data.clone();
        }
        state.not_found = rendered.not_found;
        state.global_error = rendered.global_error;
        let route = build.manifest.route(&state.route);
        let mut hints = rendered.hints;
        // A Rust loader sees the request (cookies, headers), so its output must never be shared.
        if self.loaders.contains_key(&state.route) {
            hints = CacheHints::default();
        }
        let document = Document {
            manifest: &build.manifest,
            route,
            state: &state,
            dev: self.dev,
            base_path: self.config.base_path(),
            inserted: &rendered.head,
        };
        let (html, stream) = match output {
            Output::Streamed(events) => {
                (String::new(), Some(PageStream::new(events, document.splice(), &state.pathname)))
            }
            Output::Buffered(html) => (document.render(&html), None),
        };
        let status = StatusCode::from_u16(rendered.status).unwrap_or(StatusCode::OK);
        let shared = route.filter(|r| {
            !self.dev && !rendered.incomplete && !r.id.starts_with("/_") && (r.is_cached() || hints.is_static)
        });
        let cache = match shared {
            Some(route) => cache::cache_control(hints.revalidate.or(route.revalidate)),
            None => HeaderValue::from_static(PRIVATE_CACHE),
        };
        let mut response = Response::html(html).status(status).header(header::CACHE_CONTROL, cache);
        if stream.is_some() {
            // nginx buffers proxied responses by default, which would hold the shell back.
            response.headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
        }
        Ok(Page { response, hints, data: rendered.data, incomplete: rendered.incomplete, stream })
    }

    /// Security headers, configured headers and on-the-fly gzip.
    fn decorate(&self, reply: &mut Reply, request: &http::HeaderMap) {
        let response = &mut reply.response;
        let defaults = [
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::REFERRER_POLICY, "strict-origin-when-cross-origin"),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
        ];
        for (name, value) in defaults {
            response.headers.entry(name).or_insert(HeaderValue::from_static(value));
        }
        for (name, value) in &self.config.headers {
            response.set_header(name, value);
        }
        if !self.config.server.compress
            || response.headers.contains_key(header::CONTENT_ENCODING)
            || (reply.stream.is_none() && response.body.len() < 1024)
            // A handler stream, server-sent events say, must reach the client chunk by chunk.
            || matches!(reply.stream, Some(Either::Right(_)))
        {
            return;
        }
        let content_type = response.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if !compressible(content_type) || !crate::assets::accepts(request).1 {
            return;
        }
        if let Some(Either::Left(stream)) = reply.stream.as_mut() {
            stream.gzip();
            response.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            response.headers.append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
            return;
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        if encoder.write_all(&response.body).is_ok()
            && let Ok(compressed) = encoder.finish()
        {
            response.body = compressed.into();
            response.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            response.headers.append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        }
    }
}

impl Page {
    fn cached(&self) -> CachedPage {
        CachedPage { response: self.response.clone(), data: self.data.clone() }
    }
}

impl From<Response> for Page {
    fn from(response: Response) -> Self {
        Self { response, hints: CacheHints::default(), data: None, incomplete: false, stream: None }
    }
}

impl From<CachedPage> for Page {
    fn from(page: CachedPage) -> Self {
        Self { response: page.response, hints: CacheHints::default(), data: page.data, incomplete: false, stream: None }
    }
}

/// Responses of the detecting root differ by the visitor's language and locale cookie.
fn vary_on_locale(response: &mut Response) {
    response.headers.append(header::VARY, HeaderValue::from_static("Accept-Language, Cookie"));
}

fn not_found_text() -> Response {
    Response::text("Not Found").status(StatusCode::NOT_FOUND)
}

/// Splits `path?query` at the query; the hash is dropped.
fn split_query(target: &str) -> (&str, Option<&str>) {
    let target = target.split('#').next().unwrap_or(target);
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

/// Host name of a `Host` header, without the port.
fn hostname(host: &str) -> String {
    let name = match host.find(']') {
        Some(end) if host.starts_with('[') => &host[..=end],
        _ => host.split(':').next().unwrap_or(host),
    };
    name.to_ascii_lowercase()
}

fn with_query(path: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_owned(),
    }
}

fn fill(destination: &str, params: &Params) -> String {
    if destination.starts_with("http://") || destination.starts_with("https://") {
        return destination.to_owned();
    }
    RoutePattern::parse(destination).fill(params)
}

const PRIVATE_CACHE: &str = "private, no-cache, no-store, max-age=0, must-revalidate";

/// How a no-JavaScript form submission is answered.
struct NativeForm {
    back: String,
    stateful: bool,
}

/// A form posted to a page URL by a `useActionState` permalink, recognized by React's action fields.
fn permalink_form(ctx: &Ctx) -> Option<FormData> {
    let content_type = ctx.header("content-type").unwrap_or_default();
    FormData::from_request(content_type, ctx.body()).ok().flatten().filter(|form| form.marker().action.is_some())
}

/// Where a no-JavaScript submission returns: a local `redirect` field, else a same-origin referer, else `/`.
/// Anything that could leave the site (`//host`, `javascript:`, another origin) is ignored.
fn return_path(ctx: &Ctx, requested: Option<&str>) -> String {
    // The referer is a public URL; the result is an app path, which redirects prefix with the base path.
    let referer = || {
        let uri: http::Uri = ctx.header("referer")?.parse().ok()?;
        let host = ctx.header("x-forwarded-host").or_else(|| ctx.header("host"))?;
        if uri.authority()?.as_str() != host {
            return None;
        }
        let path = ctx.config().app_path(uri.path())?;
        Some(match uri.query() {
            Some(query) => format!("{path}?{query}"),
            None => path.to_owned(),
        })
    };
    requested
        .filter(|path| is_local_path(path))
        .map(str::to_owned)
        .or_else(referer)
        .filter(|path| is_local_path(path))
        .unwrap_or_else(|| "/".into())
}

fn is_local_path(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//") && !path.starts_with("/\\") && !path.contains(char::is_control)
}

/// Rejects actions posted from another origin (CSRF).
fn same_origin(ctx: &Ctx) -> bool {
    let Some(origin) = ctx.header("origin") else {
        return true;
    };
    let host = ctx.header("x-forwarded-host").or_else(|| ctx.header("host")).unwrap_or_default();
    origin.split_once("://").map(|(_, rest)| rest) == Some(host)
}

/// Next.js-style search params: repeated keys become arrays.
fn search_params(search: &str) -> Value {
    let mut map = Map::new();
    for (key, values) in crate::form::parse_pairs(search.trim_start_matches('?').as_bytes()) {
        let value = match values.len() {
            1 => Value::String(values.into_iter().next().unwrap_or_default()),
            _ => Value::Array(values.into_iter().map(Value::String).collect()),
        };
        map.insert(key, value);
    }
    Value::Object(map)
}

fn finish(reply: Reply) -> HyperResponse {
    let Reply { response, stream } = reply;
    let body = match stream {
        Some(stream) => Either::Right(Either::Right(stream)),
        None => Either::Left(Full::new(response.body)),
    };
    let mut out = hyper::Response::new(body);
    *out.status_mut() = response.status;
    *out.headers_mut() = response.headers;
    out
}

fn sse(stream: EventStream) -> HyperResponse {
    let mut out = hyper::Response::new(Either::Right(Either::Left(stream)));
    let headers = out.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    out
}

pub(crate) async fn serve(server: Arc<Server>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;
    use hyper_util::server::graceful::GracefulShutdown;

    let builder = Builder::new(TokioExecutor::new());
    let graceful = GracefulShutdown::new();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, remote)) = accepted else { continue };
                let _ = stream.set_nodelay(true);
                let server = server.clone();
                let service = hyper::service::service_fn(move |request| {
                    let server = server.clone();
                    async move { Ok::<_, Infallible>(server.handle(request, remote).await) }
                });
                let connection = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).into_owned();
                let connection = graceful.watch(connection);
                tokio::spawn(async move {
                    let _ = connection.await;
                });
            }
            _ = &mut shutdown => break,
        }
    }
    tokio::select! {
        _ = graceful.shutdown() => {}
        _ = tokio::time::sleep(Duration::from_secs(10)) => {}
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(term) => term,
            Err(_) => return std::future::pending().await,
        };
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
