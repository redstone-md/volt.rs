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

use crate::assets::{StaticAssets, compressible, serve_public};
use crate::cache::PageCache;
use crate::config::Config;
use crate::context::{Ctx, RequestParts, State};
use crate::dev::{EventStream, Hub};
use crate::document::{Document, ERROR_ROUTE, NOT_FOUND_ROUTE, PageState, error_overlay};
use crate::error::{Error, ErrorKind, Result};
use crate::log;
use crate::manifest::{Manifest, STATIC_PREFIX};
use crate::registry::{self, ActionDef, ApiDef, BoxFuture, LoaderDef, Payload};
use crate::response::{IntoResponse, Response};
use crate::router::{Params, RoutePattern, Router};
use crate::seo::{self, SitemapEntry};
use crate::ssr::{Engine, EngineOptions};

const BODY_LIMIT: usize = 8 * 1024 * 1024;

pub type Middleware = Arc<dyn Fn(Ctx) -> BoxFuture<Option<Response>> + Send + Sync>;
type HyperResponse = hyper::Response<Either<Full<Bytes>, EventStream>>;

/// A loaded build: manifest, page routes and static assets.
struct Build {
    manifest: Manifest,
    pages: Router<usize>,
    assets: StaticAssets,
}

impl Build {
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
}

#[derive(Deserialize)]
struct RenderedRedirect {
    location: String,
    status: u16,
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
    cache: PageCache,
    hub: Option<Hub>,
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

        let server = Arc::new(Self {
            cache: PageCache::default(),
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
                        if let Ok(mut slot) = server.build.write() {
                            *slot = Arc::new(build);
                        }
                        server.engine.reload();
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
            && path == "/_volt/hmr"
            && let Some(hub) = &self.hub
        {
            return sse(hub.subscribe());
        }

        let body = match Limited::new(body, BODY_LIMIT).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return finish(Response::text("Payload Too Large").status(StatusCode::PAYLOAD_TOO_LARGE));
            }
        };
        let accept = parts.headers.clone();
        let request =
            RequestParts { method: parts.method, uri: parts.uri, headers: parts.headers, body, remote: Some(remote) };
        let ctx = Ctx::new(request, self.state.clone(), self.config.clone());

        let mut response = self.dispatch(&ctx).await;
        ctx.take_response().apply(&mut response);
        self.decorate(&mut response, &accept);
        if self.dev && !path.starts_with(STATIC_PREFIX) {
            log::request(method.as_str(), &path, response.status.as_u16(), started.elapsed());
        }
        finish(response)
    }

    async fn dispatch(&self, ctx: &Ctx) -> Response {
        let path = ctx.path().to_owned();
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
            return self.data(ctx).await;
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
        if !same_origin(ctx) {
            return Error::forbidden("cross-origin action call rejected").into_response();
        }
        let Some(action) = self.actions.get(name) else {
            return Error::new(StatusCode::NOT_FOUND, format!("unknown action `{name}`")).into_response();
        };
        let content_type = ctx.header("content-type").unwrap_or_default();
        let form = content_type.starts_with("application/x-www-form-urlencoded");
        if !form && content_type.starts_with("multipart/") {
            return Error::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "multipart bodies are not supported yet")
                .into_response();
        }
        let payload = if form { Payload::Form(ctx.body().clone()) } else { Payload::Json(ctx.body().clone()) };
        let result = (action.handler)(ctx.clone(), payload).await;
        if form {
            return self.form_result(ctx, result);
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

    /// Answers a no-JavaScript form submission with a redirect back to the page.
    fn form_result(&self, ctx: &Ctx, result: Result<Value>) -> Response {
        let back = crate::form::parse_pairs(ctx.body())
            .into_iter()
            .find(|(k, _)| k == "redirect")
            .and_then(|(_, v)| v.into_iter().next())
            .or_else(|| ctx.header("referer").map(str::to_owned))
            .unwrap_or_else(|| "/".into());
        match result {
            Ok(_) => Response::redirect(StatusCode::SEE_OTHER, &back),
            Err(error) => match error.kind() {
                ErrorKind::Redirect(location) => Response::redirect(StatusCode::SEE_OTHER, location),
                _ => {
                    let message = crate::document::escape_html(error.client_message(self.dev));
                    Response::html(format!(
                        "<!DOCTYPE html><meta charset=\"utf-8\"><title>Error</title><p>{message}</p><p><a href=\"{}\">Back</a></p>",
                        crate::document::escape_html(&back)
                    ))
                    .status(error.status())
                }
            },
        }
    }

    async fn data(&self, ctx: &Ctx) -> Response {
        let target = ctx.query_param("path").unwrap_or_else(|| "/".into());
        let (pathname, search) = match target.split_once('?') {
            Some((p, s)) => (p.to_owned(), format!("?{s}")),
            None => (target.clone(), String::new()),
        };
        let pathname = self.rewrite(&pathname);
        let state = match self.resolve(ctx, &pathname, &search).await {
            Ok(state) => state,
            Err(error) => match error.kind() {
                ErrorKind::Redirect(location) => {
                    return Response::json(&json!({ "redirect": location }));
                }
                _ => self.failure_state(&pathname, &search, &error),
            },
        };
        Response::json(&state).header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
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
        let mut entries: Vec<SitemapEntry> = build
            .manifest
            .routes
            .iter()
            .filter(|r| !r.id.starts_with("/_") && !RoutePattern::parse(&r.id).is_dynamic())
            .map(|r| SitemapEntry::new(r.id.clone()))
            .collect();
        for provider in registry::sitemaps() {
            match (provider.handler)(ctx.clone()).await {
                Ok(more) => entries.extend(more),
                Err(error) => log::error(&format!("sitemap provider failed: {}", error.message())),
            }
        }
        entries
    }

    async fn page(&self, ctx: &Ctx, path: &str) -> Response {
        if !matches!(*ctx.method(), Method::GET | Method::HEAD) {
            return Response::text("Method Not Allowed").status(StatusCode::METHOD_NOT_ALLOWED);
        }
        let key = format!("{path}?{}", ctx.query_string());
        let build = self.current();
        let entry = build.pages.find(path).map(|(_, index, _)| &build.manifest.routes[*index]);
        let cacheable = !self.dev && entry.is_some_and(|e| e.prerendered.is_some() || e.revalidate.is_some());
        if cacheable && let Some(hit) = self.cache.get(&key) {
            return hit;
        }
        let search = ctx.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
        let state = match self.resolve(ctx, path, &search).await {
            Ok(state) => state,
            Err(error) => match error.kind() {
                ErrorKind::Redirect(location) => {
                    return Response::redirect(error.status(), location);
                }
                _ => self.failure_state(path, &search, &error),
            },
        };
        let response = self.render(&build, state).await;
        if cacheable && response.status == StatusCode::OK {
            let ttl = entry.and_then(|e| e.revalidate).map(Duration::from_secs);
            self.cache.put(key, &response, ttl);
        }
        response
    }

    /// Renders routes ahead of time; used by `volt build` for static pages.
    pub async fn prerender(&self, routes: &[String]) -> Vec<(String, Result<String>)> {
        let mut out = Vec::with_capacity(routes.len());
        for route in routes {
            let uri = route.parse().unwrap_or_default();
            let request = RequestParts {
                method: Method::GET,
                uri,
                headers: http::HeaderMap::new(),
                body: Bytes::new(),
                remote: None,
            };
            let ctx = Ctx::new(request, self.state.clone(), self.config.clone());
            let response = self.page(&ctx, route).await;
            let result = match response.status {
                StatusCode::OK => Ok(String::from_utf8_lossy(&response.body).into_owned()),
                status => Err(Error::internal(format!("prerender returned {status}"))),
            };
            out.push((route.clone(), result));
        }
        out
    }

    /// Matches the route and runs its loader.
    async fn resolve(&self, ctx: &Ctx, pathname: &str, search: &str) -> Result<PageState> {
        let build = self.current();
        let Some((pattern, _, params)) = build.pages.find(pathname) else {
            return Err(Error::not_found());
        };
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
            build_id: self.current().manifest.build_id.clone(),
        }
    }

    async fn render(&self, build: &Build, state: PageState) -> Response {
        match self.render_once(build, &state).await {
            Ok(response) => response,
            Err(error) if self.dev => {
                log::error(error.message());
                Response::html(error_overlay("Render failed", error.message()))
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
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
                    Ok(response) => response,
                    Err(_) => Response::html("<!DOCTYPE html><title>500</title><h1>500 Internal Server Error</h1>")
                        .status(StatusCode::INTERNAL_SERVER_ERROR),
                }
            }
        }
    }

    async fn render_once(&self, build: &Build, state: &PageState) -> Result<Response> {
        let payload = serde_json::to_string(state)?;
        let output = self.engine.call("render", payload).await?;
        let rendered: Rendered = serde_json::from_str(&output)?;
        if let Some(redirect) = rendered.redirect {
            let status = StatusCode::from_u16(redirect.status).unwrap_or(StatusCode::TEMPORARY_REDIRECT);
            return Ok(Response::redirect(status, &redirect.location));
        }
        let route = build.manifest.route(&state.route);
        let mut state = state.clone();
        state.status = rendered.status;
        let html = Document { manifest: &build.manifest, route, state: &state, dev: self.dev }.render(&rendered.html);
        let status = StatusCode::from_u16(rendered.status).unwrap_or(StatusCode::OK);
        let cache = if route.is_some_and(|r| r.prerendered.is_some()) {
            "public, max-age=0, s-maxage=31536000, stale-while-revalidate"
        } else {
            "private, no-cache, no-store, max-age=0, must-revalidate"
        };
        Ok(Response::html(html).status(status).header(header::CACHE_CONTROL, HeaderValue::from_static(cache)))
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

fn not_found_text() -> Response {
    Response::text("Not Found").status(StatusCode::NOT_FOUND)
}

fn fill(destination: &str, params: &Params) -> String {
    if destination.starts_with("http://") || destination.starts_with("https://") {
        return destination.to_owned();
    }
    RoutePattern::parse(destination).fill(params)
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
