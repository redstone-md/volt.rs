use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use serde::de::DeserializeOwned;

use crate::cache::{PageCache, Target};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::form::FormData;
use crate::response::{Cookie, Response};
use crate::router::Params;

/// Application state registered with [`crate::App::state`].
#[derive(Default)]
pub struct State {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl State {
    pub(crate) fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.values.insert(TypeId::of::<T>(), Box::new(value));
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.values.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }
}

pub(crate) struct RequestParts {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub remote: Option<SocketAddr>,
    pub started: Instant,
}

/// Response hint listing revalidated paths; the client runtime drops matching cached route data.
pub const REVALIDATE_HEADER: &str = "x-volt-revalidate";

/// Revalidates every path.
const REVALIDATE_ALL: &str = "*";

/// Path characters escaped in the revalidate header: controls, non-ASCII (implicitly) and the list separator.
const HEADER_UNSAFE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS.add(b',').add(b'%').add(b' ');

#[derive(Default)]
pub(crate) struct ResponseParts {
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub status: Option<StatusCode>,
    pub revalidate: Vec<String>,
}

impl ResponseParts {
    pub fn apply(self, response: &mut Response) {
        if let Some(status) = self.status {
            response.status = status;
        }
        for (name, value) in self.headers {
            response.headers.append(name, value);
        }
        if self.revalidate.is_empty() {
            return;
        }
        let list: Vec<String> = self
            .revalidate
            .iter()
            .map(|path| percent_encoding::utf8_percent_encode(path, HEADER_UNSAFE).to_string())
            .collect();
        if let Ok(value) = HeaderValue::from_str(&list.join(",")) {
            response.headers.insert(HeaderName::from_static(REVALIDATE_HEADER), value);
        }
    }
}

/// Per-request context passed to actions, loaders, API handlers and middleware.
#[derive(Clone)]
pub struct Ctx {
    request: Arc<RequestParts>,
    params: Arc<Params>,
    state: Arc<State>,
    config: Arc<Config>,
    cache: Arc<PageCache>,
    response: Arc<Mutex<ResponseParts>>,
}

impl Ctx {
    pub(crate) fn new(request: RequestParts, state: Arc<State>, config: Arc<Config>, cache: Arc<PageCache>) -> Self {
        Self { request: Arc::new(request), params: Arc::default(), state, config, cache, response: Arc::default() }
    }

    pub(crate) fn with_params(&self, params: Params) -> Self {
        Self { params: Arc::new(params), ..self.clone() }
    }

    pub(crate) fn take_response(&self) -> ResponseParts {
        std::mem::take(&mut *self.lock_response())
    }

    fn lock_response(&self) -> MutexGuard<'_, ResponseParts> {
        self.response.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn method(&self) -> &Method {
        &self.request.method
    }

    pub fn uri(&self) -> &Uri {
        &self.request.uri
    }

    pub fn path(&self) -> &str {
        self.request.uri.path()
    }

    pub fn query_string(&self) -> &str {
        self.request.uri.query().unwrap_or_default()
    }

    /// Deserializes the query string with the same lenient rules as forms.
    pub fn query<T: DeserializeOwned>(&self) -> Result<T> {
        crate::form::from_bytes(self.query_string().as_bytes()).map_err(|e| Error::bad_request(e.to_string()))
    }

    pub fn query_param(&self, name: &str) -> Option<String> {
        crate::form::parse_pairs(self.query_string().as_bytes())
            .into_iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, values)| values.into_iter().next())
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name)
    }

    /// Parses a route param, answering 404 when it is missing or malformed.
    pub fn param_as<T: FromStr>(&self, name: &str) -> Result<T> {
        self.param(name).and_then(|v| v.parse().ok()).ok_or_else(Error::not_found)
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.request.headers
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.request.headers.get(name).and_then(|v| v.to_str().ok())
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.request
            .headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| percent_encoding::percent_decode_str(value).decode_utf8_lossy().into_owned())
    }

    pub fn body(&self) -> &Bytes {
        &self.request.body
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.request.body).map_err(|e| Error::bad_request(format!("invalid JSON body: {e}")))
    }

    /// Deserializes an urlencoded or multipart body (files included) with the lenient form rules.
    pub fn form<T: DeserializeOwned>(&self) -> Result<T> {
        self.multipart()?.decode().map_err(|e| Error::bad_request(e.to_string()))
    }

    /// Parses a `multipart/form-data` (or urlencoded) body into text fields and files.
    pub fn multipart(&self) -> Result<FormData> {
        let content_type = self.header("content-type").unwrap_or_default();
        match FormData::from_request(content_type, &self.request.body) {
            Ok(Some(form)) => Ok(form),
            Ok(None) => Err(Error::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "expected a form body")),
            Err(error) => Err(Error::bad_request(error.to_string())),
        }
    }

    /// Time since the request arrived.
    pub fn elapsed(&self) -> Duration {
        self.request.started.elapsed()
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.request.remote
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns state registered with [`crate::App::state`]. Panics if it was not registered.
    pub fn state<T: Send + Sync + 'static>(&self) -> &T {
        self.try_state().unwrap_or_else(|| panic!("state `{}` is not registered", std::any::type_name::<T>()))
    }

    pub fn try_state<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.state.get()
    }

    pub fn set_cookie(&self, cookie: Cookie) {
        if let Ok(value) = HeaderValue::from_str(&cookie.to_header()) {
            self.lock_response().headers.push((header::SET_COOKIE, value));
        }
    }

    pub fn remove_cookie(&self, name: &str) {
        self.set_cookie(Cookie::removal(name));
    }

    pub fn set_header(&self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            self.lock_response().headers.push((name, value));
        }
    }

    pub fn set_status(&self, status: StatusCode) {
        self.lock_response().status = Some(status);
    }

    /// Drops every cached page on the server and every route's cached data on the client,
    /// and makes the client re-run the current route's loader.
    pub fn revalidate(&self) -> usize {
        self.hint(REVALIDATE_ALL.to_owned());
        Target::Prefix("/".to_owned()).apply(&self.cache)
    }

    /// Purges the cached page at `path` (or every page of a pattern like `/blog/[slug]`) on this
    /// server and tells the client runtime to drop its matching route data. See [`crate::revalidate_path`].
    pub fn revalidate_path(&self, path: &str) -> usize {
        self.hint(path.to_owned());
        Target::Path(path.to_owned()).apply(&self.cache)
    }

    /// Purges cached pages at or below `prefix` on this server and the client. See [`crate::revalidate_prefix`].
    pub fn revalidate_prefix(&self, prefix: &str) -> usize {
        self.hint(format!("{}/[[...path]]", prefix.trim_end_matches('/')));
        Target::Prefix(prefix.to_owned()).apply(&self.cache)
    }

    fn hint(&self, path: String) {
        let mut response = self.lock_response();
        if !response.revalidate.contains(&path) {
            response.revalidate.push(path);
        }
    }

    pub(crate) fn sets_cookies(&self) -> bool {
        self.lock_response().headers.iter().any(|(name, _)| name == header::SET_COOKIE)
    }
}
