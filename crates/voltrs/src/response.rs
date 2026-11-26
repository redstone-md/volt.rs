use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use serde::Serialize;

use crate::error::{Error, ErrorKind};

/// An HTTP response produced by handlers and the framework itself.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Default for Response {
    fn default() -> Self {
        Self::new(StatusCode::OK, Bytes::new())
    }
}

impl Response {
    pub fn new(status: StatusCode, body: impl Into<Bytes>) -> Self {
        Self { status, headers: HeaderMap::new(), body: body.into() }
    }

    pub fn with_type(status: StatusCode, content_type: &'static str, body: impl Into<Bytes>) -> Self {
        Self::new(status, body).header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self::with_type(StatusCode::OK, "text/plain; charset=utf-8", body.into())
    }

    pub fn html(body: impl Into<String>) -> Self {
        Self::with_type(StatusCode::OK, "text/html; charset=utf-8", body.into())
    }

    pub fn json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self::with_type(StatusCode::OK, "application/json", body),
            Err(error) => Error::from(error).into_response(),
        }
    }

    pub fn redirect(status: StatusCode, location: &str) -> Self {
        let mut response = Self::new(status, Bytes::new());
        if let Ok(value) = HeaderValue::from_str(location) {
            response.headers.insert(header::LOCATION, value);
        }
        response
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status, Bytes::new())
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn set_header(&mut self, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            self.headers.insert(name, value);
        }
    }
}

/// Converts handler return values into a [`Response`].
pub trait IntoResponse {
    fn into_response(self) -> Response;
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        Response::empty(StatusCode::NO_CONTENT)
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        Response::text(self)
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        Response::text(self)
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        Response::empty(self)
    }
}

impl<T: IntoResponse> IntoResponse for (StatusCode, T) {
    fn into_response(self) -> Response {
        self.1.into_response().status(self.0)
    }
}

impl<T: IntoResponse, E: Into<Error>> IntoResponse for Result<T, E> {
    fn into_response(self) -> Response {
        match self {
            Ok(value) => value.into_response(),
            Err(error) => error.into().into_response(),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self.kind() {
            ErrorKind::Redirect(location) => Response::redirect(self.status(), location),
            _ => Response::json(&serde_json::json!({ "error": self.to_json(crate::config::is_dev()) }))
                .status(self.status()),
        }
    }
}

/// JSON response body.
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        Response::json(&self.0)
    }
}

/// HTML response body.
#[derive(Debug, Clone)]
pub struct Html<T>(pub T);

impl<T: Into<String>> IntoResponse for Html<T> {
    fn into_response(self) -> Response {
        Response::html(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// A `Set-Cookie` builder.
#[derive(Debug, Clone)]
pub struct Cookie {
    name: String,
    value: String,
    path: Option<String>,
    domain: Option<String>,
    max_age: Option<i64>,
    http_only: bool,
    secure: bool,
    same_site: Option<SameSite>,
}

impl Cookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            path: Some("/".into()),
            domain: None,
            max_age: None,
            http_only: true,
            secure: false,
            same_site: Some(SameSite::Lax),
        }
    }

    /// A cookie that expires immediately.
    pub fn removal(name: impl Into<String>) -> Self {
        Self::new(name, "").max_age(0)
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn max_age(mut self, seconds: i64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn http_only(mut self, value: bool) -> Self {
        self.http_only = value;
        self
    }

    pub fn secure(mut self, value: bool) -> Self {
        self.secure = value;
        self
    }

    pub fn same_site(mut self, value: SameSite) -> Self {
        self.same_site = Some(value);
        self
    }

    pub fn to_header(&self) -> String {
        let encode = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
        let mut out = format!("{}={}", self.name, encode(&self.value));
        if let Some(path) = &self.path {
            out.push_str(&format!("; Path={path}"));
        }
        if let Some(domain) = &self.domain {
            out.push_str(&format!("; Domain={domain}"));
        }
        if let Some(age) = self.max_age {
            out.push_str(&format!("; Max-Age={age}"));
        }
        if self.http_only {
            out.push_str("; HttpOnly");
        }
        if self.secure {
            out.push_str("; Secure");
        }
        if let Some(same_site) = self.same_site {
            out.push_str(match same_site {
                SameSite::Strict => "; SameSite=Strict",
                SameSite::Lax => "; SameSite=Lax",
                SameSite::None => "; SameSite=None",
            });
        }
        out
    }
}
