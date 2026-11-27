//! Request dispatch: static assets, actions, data, API handlers and rendered pages.

use std::collections::HashMap;
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
use crate::assets::{StaticAssets, compressible, serve_public};
use crate::cache::{self, CachedPage, Lookup, PageCache};
use crate::config::Config;
use crate::context::{Ctx, RequestParts, State};
use crate::dev::{EventStream, Hub};
use crate::document::{Document, ERROR_ROUTE, NOT_FOUND_ROUTE, PageState, Site, error_overlay};
use crate::error::{Error, ErrorKind, Result};
use crate::flash::ActionFlash;
use crate::form::FormData;
use crate::log;
use crate::manifest::{Manifest, RouteEntry, STATIC_PREFIX};
use crate::registry::{self, ActionDef, ApiDef, BoxFuture, LoaderDef, Payload};
use crate::response::{IntoResponse, Response};
use crate::router::{Params, RoutePattern, Router};
use crate::seo::{self, SitemapEntry};
use crate::ssr::{Engine, EngineOptions};

pub type Middleware = Arc<dyn Fn(Ctx) -> BoxFuture<Option<Response>> + Send + Sync>;
type HyperResponse = hyper::Response<Either<Full<Bytes>, EventStream>>;

/// A loaded build: manifest, page routes and static assets.
struct Build {
    manifest: Manifest,
    pages: Router<usize>,
    assets: StaticAssets,
}

impl Build {
    fn entry(&self, path: &str) -> Option<&RouteEntry> {
        self.pages.find(path).map(|(_, index, _)| &self.manifest.routes[*index])
    }

    fn load(out_dir: &std::path::Path) -> Result<Self> {
        let manifest = Manifest::load(out_dir)
            .map_err(|e| Error::internal(format!("no build found in {} ({e}); run `volt build`", out_dir.display())))?;
        let pages = manifest
            .routes
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.id.starts_with("/_"))
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        let assets = StaticAssets::load(&out_dir.join("static"));
        Ok(Self { manifest, pages, assets })
    }
}

#[derive(Deserialize)]
struct Rendered {
    html: String,
    status: u16,
    redirect: Option<RenderedRedirect>,
    /// Set when rendering fell back to another route, such as not-found.
    #[serde(default)]
    route: Option<String>,
    /// Pages Router props that replace the loader data in the serialized state.
    #[serde(default)]
    data: Option<Value>,
    #[serde(flatten)]
    hints: CacheHints,
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

/// A rendered page with the caching hints and props its data functions returned.
struct Page {
    response: Response,
    hints: CacheHints,
    data: Option<Value>,
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
    #[cfg(feature = "images")]
    images: crate::images::Images,
    dev: bool,
}

impl Server {
    pub fn new(config: Config, state: State, middleware: Vec<Middleware>) -> Result<Arc<Self>> {
        let dev = crate::config::is_dev();
        let out_dir = config.out_dir();
        let build = Build::load(&out_dir)?;
        let engine = Engine::start(EngineOptions {
            bundle: out_dir.join(&build.manifest.server),
            workers: config.workers(),
            memory_limit: config.server.memory_limit_mb * 1024 * 1024,
            dev,
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
            cache: PageCache::shared(config.server.page_cache_mb * 1024 * 1024),
            hub: dev.then(Hub::default),
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
            server.cache.preload(&server.current().manifest.routes, &out_dir);
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
                        let previous = server.current();
                        let css_only = previous.manifest.server == build.manifest.server
                            && previous.manifest.client.entry == build.manifest.client.entry
                            && previous.manifest.client.css != build.manifest.client.css;
                        let bundle = out_dir.join(&build.manifest.server);
                        if let Ok(mut slot) = server.build.write() {
                            *slot = Arc::new(build);
                        }
                        server.engine.reload(bundle);
                        if let Some(hub) = &server.hub {
                            hub.broadcast(if css_only { "css" } else { "reload" }, "");
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
                return finish(Response::text("Payload Too Large").status(StatusCode::PAYLOAD_TOO_LARGE));
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

        let mut response = self.dispatch(&ctx).await;
        ctx.take_response().apply(&mut response);
        self.localize_redirect(&mut response);
        self.decorate(&mut response, &accept);
        if self.dev && !path.starts_with(&self.config.static_prefix()) {
            log::request(method.as_str(), &path, response.status.as_u16(), started.elapsed());
        }
        finish(response)
    }

    async fn dispatch(self: &Arc<Self>, ctx: &Ctx) -> Response {
        let Some(path) = self.config.app_path(ctx.path()).map(str::to_owned) else {
            return not_found_text();
        };
        if let Some(response) = self.normalize_slash(ctx, &path) {
            return response;
        }
        if let Some(response) = self.redirect(&path) {
            return response;
        }
        for middleware in &self.middleware {
            if let Some(response) = middleware(ctx.clone()).await {
                return response;
            }
        }
        if let Some(asset) = path.strip_prefix(STATIC_PREFIX) {
            return self.current().assets.serve(asset, ctx.headers()).unwrap_or_else(not_found_text);
        }
        if let Some(name) = path.strip_prefix("/_volt/action/") {
            return self.action(ctx, name).await;
        }
        if path == "/_volt/data" {
            return self.data(ctx, &ctx.query_param("path").unwrap_or_else(|| "/".into())).await;
        }
        if let Some(stem) = path.strip_prefix("/_volt/data/").and_then(|p| p.strip_suffix(".json")) {
            let pathname = if stem == "index" { "/".to_owned() } else { format!("/{stem}") };
            let target = match ctx.uri().query() {
                Some(query) => format!("{pathname}?{query}"),
                None => pathname,
            };
            return self.data(ctx, &target).await;
        }
        #[cfg(feature = "images")]
        if path == "/_volt/image" && self.config.images.optimize {
            return self.images.serve(ctx.query_string()).await;
        }
        if let Some(error) = self.current().manifest.error.as_deref().filter(|_| self.dev) {
            return Response::html(error_overlay("Build failed", error, self.config.base_path()))
                .status(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let path = self.rewrite(&path);
        if let Some(response) = self.api(ctx, &path).await {
            return response;
        }
        if matches!(*ctx.method(), Method::GET | Method::HEAD) {
            if let Some(response) = serve_public(&self.config.public_dir(), &path, ctx.headers()).await {
                return response;
            }
            if let Some(response) = self.seo(ctx, &path).await {
                return response;
            }
        }
        self.page(ctx, &path).await
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

    fn redirect(&self, path: &str) -> Option<Response> {
        self.config.redirects.iter().find_map(|rule| {
            let params = RoutePattern::parse(&rule.source).matches(path)?;
            let status = if rule.permanent { StatusCode::PERMANENT_REDIRECT } else { StatusCode::TEMPORARY_REDIRECT };
            Some(Response::redirect(status, &fill(&rule.destination, &params)))
        })
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

    async fn action(&self, ctx: &Ctx, name: &str) -> Response {
        if *ctx.method() != Method::POST {
            return Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED);
        }
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
            return Response::redirect(StatusCode::SEE_OTHER, location);
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
        let (pathname, search) = match target.split_once('?') {
            Some((p, s)) => (p.to_owned(), format!("?{s}")),
            None => (target.to_owned(), String::new()),
        };
        let pathname = self.rewrite(&pathname);
        let resolved = match self.resolve(ctx, &pathname, &search).await {
            Ok(state) => match self.static_props(ctx, &state).await {
                Some(data) => Ok(PageState { data, ..state }),
                None => self.with_props(state).await,
            },
            Err(error) => Err(error),
        };
        match self.settle(resolved, &pathname, &search) {
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

    async fn seo(&self, ctx: &Ctx, path: &str) -> Option<Response> {
        let seo = &self.config.seo;
        match path {
            "/robots.txt" if seo.robots => Some(Response::text(seo::robots(&self.config))),
            "/sitemap.xml" if seo.sitemap => {
                let xml = seo::sitemap(&self.config, &self.sitemap_entries(ctx).await);
                Some(Response::with_type(StatusCode::OK, "application/xml", xml))
            }
            "/llms.txt" if seo.llms => Some(Response::text(seo::llms(&self.config, &self.sitemap_entries(ctx).await))),
            _ => None,
        }
    }

    async fn sitemap_entries(&self, ctx: &Ctx) -> Vec<SitemapEntry> {
        let build = self.current();
        let mut entries: Vec<SitemapEntry> = Vec::new();
        for route in build.manifest.routes.iter().filter(|r| !r.id.starts_with("/_")) {
            if !route.paths.is_empty() {
                entries.extend(route.paths.iter().map(|p| SitemapEntry::new(p.clone())));
            } else if !RoutePattern::parse(&route.id).is_dynamic() {
                entries.push(SitemapEntry::new(route.id.clone()));
            }
        }
        for provider in registry::sitemaps() {
            match (provider.handler)(ctx.clone()).await {
                Ok(more) => entries.extend(more),
                Err(error) => log::error(&format!("sitemap provider failed: {}", error.message())),
            }
        }
        entries
    }

    async fn page(self: &Arc<Self>, ctx: &Ctx, path: &str) -> Response {
        if *ctx.method() == Method::POST
            && let Some(form) = permalink_form(ctx)
            && let Some(name) = form.marker().action.clone()
        {
            let page = ctx.config().app_path(ctx.uri().path()).unwrap_or(path);
            let page = match ctx.uri().query() {
                Some(query) => format!("{page}?{query}"),
                None => page.to_owned(),
            };
            return self.run_action(ctx, &name, Some(form), Some(&page)).await;
        }
        if matches!(*ctx.method(), Method::GET | Method::HEAD)
            && let Some(flash) = ActionFlash::take(ctx)
        {
            return self.flash_page(ctx, path, flash).await;
        }
        self.serve_page(ctx, path).await.response
    }

    /// Renders a page for one visitor with the result of their no-JavaScript action submission, bypassing the cache.
    async fn flash_page(&self, ctx: &Ctx, path: &str, flash: Value) -> Response {
        let build = self.current();
        let search = ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
        let mut response = match self.resolve_page(ctx, path, &search).await {
            Resolved::Page(mut state) => {
                state.action = Some(flash);
                self.render(&build, *state).await.response
            }
            Resolved::Redirect(status, location) => return Response::redirect(status, &location),
        };
        response.headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(PRIVATE_CACHE));
        response
    }

    async fn serve_page(self: &Arc<Self>, ctx: &Ctx, path: &str) -> Page {
        if !matches!(*ctx.method(), Method::GET | Method::HEAD) {
            return Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED).into();
        }
        let build = self.current();
        let entry = build.entry(path);
        let is_static = entry.is_some_and(|e| self.is_static_route(e));
        // `getStaticProps` cannot see the query, so one entry per pathname serves every query string.
        let (query, search) = if is_static {
            ("", String::new())
        } else {
            (ctx.query_string(), ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default())
        };
        let cacheable = !self.dev && entry.is_some_and(|e| e.is_cached() || is_static);
        if !cacheable {
            return self.generate(ctx, &build, path, &search).await;
        }
        let key = cache::key(path, query);
        match self.cache.get(&key) {
            Lookup::Fresh(page) => return page.into(),
            Lookup::Stale { page, refresh } => {
                if refresh {
                    self.refresh(key, path.to_owned(), search);
                }
                return page.into();
            }
            Lookup::Miss => {}
        }
        let ticket = self.cache.ticket();
        let page = self.generate(ctx, &build, path, &search).await;
        if page.response.status == StatusCode::OK {
            self.cache.put(key, &page.cached(), Self::ttl(&build, path, &page), ticket);
        }
        page
    }

    /// Cache lifetime of a rendered page: `revalidate` from `getStaticProps`, else from the route.
    fn ttl(build: &Build, path: &str, page: &Page) -> Option<Duration> {
        page.hints.revalidate.or_else(|| build.entry(path).and_then(|e| e.revalidate)).map(Duration::from_secs)
    }

    /// Regenerates a stale page in the background while the stale copy keeps being served.
    fn refresh(self: &Arc<Self>, key: String, path: String, search: String) {
        let server = self.clone();
        tokio::spawn(async move {
            let ctx = server.synthetic(&format!("{path}{search}"));
            let build = server.current();
            let ticket = server.cache.ticket();
            let page = server.generate(&ctx, &build, &path, &search).await;
            match page.response.status {
                StatusCode::OK => server.cache.put(key, &page.cached(), Self::ttl(&build, &path, &page), ticket),
                StatusCode::NOT_FOUND => server.cache.remove(&key),
                status => {
                    log::warn(&format!("revalidating {path} returned {status}; serving the stale page"));
                    server.cache.release(&key);
                }
            }
        });
    }

    /// Resolves and renders a page without consulting the cache.
    async fn generate(&self, ctx: &Ctx, build: &Build, path: &str, search: &str) -> Page {
        match self.resolve_page(ctx, path, search).await {
            Resolved::Page(state) => self.render(build, *state).await,
            Resolved::Redirect(status, location) => Response::redirect(status, &location).into(),
        }
    }

    /// A GET request without headers, used for prerendering and background regeneration.
    fn synthetic(&self, app_path: &str) -> Ctx {
        let request = RequestParts {
            method: Method::GET,
            uri: self.config.public_path(app_path).parse().unwrap_or_default(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
            remote: None,
            started: Instant::now(),
        };
        Ctx::new(request, self.state.clone(), self.config.clone(), self.cache.clone())
    }

    /// Renders one concrete path ahead of time, with the route data the client navigates with.
    pub(crate) async fn prerender(&self, path: &str) -> Prerendered {
        let ctx = self.synthetic(path);
        let build = self.current();
        match self.resolve_page(&ctx, path, "").await {
            Resolved::Page(mut state) => {
                let page = self.render(&build, (*state).clone()).await;
                if let Some(data) = page.data {
                    state.data = data;
                }
                Prerendered {
                    path: path.to_owned(),
                    status: page.response.status,
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
        let page = self.render(&self.current(), state).await;
        let html = String::from_utf8_lossy(&page.response.body).into_owned();
        Prerendered { path: path.to_owned(), status: page.response.status, html, data, revalidate: None }
    }

    /// `robots.txt`, `sitemap.xml` and `llms.txt` as enabled in `[seo]`.
    pub(crate) async fn generated_files(&self) -> Vec<(String, Bytes)> {
        let ctx = self.synthetic("/");
        let mut files = Vec::new();
        for name in ["/robots.txt", "/sitemap.xml", "/llms.txt"] {
            if let Some(response) = self.seo(&ctx, name).await {
                files.push((name.trim_start_matches('/').to_owned(), response.body));
            }
        }
        files
    }

    async fn resolve_page(&self, ctx: &Ctx, path: &str, search: &str) -> Resolved {
        let resolved = self.resolve(ctx, path, search).await;
        self.settle(resolved, path, search)
    }

    /// Turns a loader error into a redirect or the error page state.
    fn settle(&self, resolved: Result<PageState>, path: &str, search: &str) -> Resolved {
        match resolved {
            Ok(state) => Resolved::Page(Box::new(state)),
            Err(error) => match error.kind() {
                ErrorKind::Redirect(location) => Resolved::Redirect(error.status(), location.clone()),
                _ => Resolved::Page(Box::new(self.failure_state(path, search, &error))),
            },
        }
    }

    /// Matches the route and runs its loader.
    async fn resolve(&self, ctx: &Ctx, pathname: &str, search: &str) -> Result<PageState> {
        let build = self.current();
        let Some((pattern, index, params)) = build.pages.find(pathname) else {
            return Err(Error::not_found());
        };
        if !self.dev && !build.manifest.routes[*index].accepts(pathname) {
            return Err(Error::not_found());
        }
        let route = pattern.as_str().to_owned();
        let ctx = ctx.with_params(params.clone());
        let data = match self.loaders.get(&route) {
            Some(loader) => (loader.handler)(ctx).await?,
            None => Value::Null,
        };
        Ok(self.state(route, pathname, search, params.to_json(), data, 200, None))
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
            build_id: self.current().manifest.build_id.clone(),
            site: Site { name: self.config.site.name.clone(), url: self.config.site_url().to_owned() },
            export: crate::config::is_export(),
        }
    }

    async fn render(&self, build: &Build, state: PageState) -> Page {
        match self.render_once(build, &state).await {
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
                match self.render_once(build, &fallback).await {
                    Ok(page) => page,
                    Err(_) => Response::html("<!DOCTYPE html><title>500</title><h1>500 Internal Server Error</h1>")
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .into(),
                }
            }
        }
    }

    async fn render_once(&self, build: &Build, state: &PageState) -> Result<Page> {
        let payload = serde_json::to_string(state)?;
        let output = self.engine.call("render", payload).await?;
        let rendered: Rendered = serde_json::from_str(&output)?;
        if let Some(redirect) = rendered.redirect {
            let status = StatusCode::from_u16(redirect.status).unwrap_or(StatusCode::TEMPORARY_REDIRECT);
            return Ok(Response::redirect(status, &redirect.location).into());
        }
        let mut state = state.clone();
        state.status = rendered.status;
        if let Some(route) = rendered.route {
            state.route = route;
        }
        if let Some(data) = &rendered.data {
            state.data = data.clone();
        }
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
        };
        let html = document.render(&rendered.html);
        let status = StatusCode::from_u16(rendered.status).unwrap_or(StatusCode::OK);
        let shared = route.filter(|r| !self.dev && !r.id.starts_with("/_") && (r.is_cached() || hints.is_static));
        let cache = match shared {
            Some(route) => cache::cache_control(hints.revalidate.or(route.revalidate)),
            None => HeaderValue::from_static(PRIVATE_CACHE),
        };
        let response = Response::html(html).status(status).header(header::CACHE_CONTROL, cache);
        Ok(Page { response, hints, data: rendered.data })
    }

    /// Security headers, configured headers and on-the-fly gzip.
    fn decorate(&self, response: &mut Response, request: &http::HeaderMap) {
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
            || response.body.len() < 1024
        {
            return;
        }
        let content_type = response.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
        if !compressible(content_type) || !crate::assets::accepts(request).1 {
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
        Self { response, hints: CacheHints::default(), data: None }
    }
}

impl From<CachedPage> for Page {
    fn from(page: CachedPage) -> Self {
        Self { response: page.response, hints: CacheHints::default(), data: page.data }
    }
}

fn not_found_text() -> Response {
    Response::text("Not Found").status(StatusCode::NOT_FOUND)
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

fn finish(response: Response) -> HyperResponse {
    let mut out = hyper::Response::new(Either::Left(Full::new(response.body)));
    *out.status_mut() = response.status;
    *out.headers_mut() = response.headers;
    out
}

fn sse(stream: EventStream) -> HyperResponse {
    let mut out = hyper::Response::new(Either::Right(stream));
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
