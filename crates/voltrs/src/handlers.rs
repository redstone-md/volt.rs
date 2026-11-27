//! The exchange with route handlers (`route.ts`) and `middleware.ts` running in QuickJS.

use bytes::BytesMut;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::context::Ctx;
use crate::error::{Error, Result};
use crate::response::Response;
use crate::ssr::StreamEvent;

/// Headers by which `NextResponse.next()` and `NextResponse.rewrite()` speak to the server, as in Next.js.
const NEXT: &str = "x-middleware-next";
const REWRITE: &str = "x-middleware-rewrite";
const OVERRIDE: &str = "x-middleware-override-headers";
const REQUEST_PREFIX: &str = "x-middleware-request-";

/// The request handed to `__VOLT__.handle` and `__VOLT__.middleware`; the body travels as `sink.body`.
#[derive(Debug, Serialize)]
pub(crate) struct ScriptRequest<'a> {
    pub method: &'a str,
    /// Absolute URL as the client sees it, base path included.
    pub url: String,
    /// App path matched against routes and matchers.
    pub path: &'a str,
    pub headers: Vec<(&'a str, &'a str)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<&'a str>,
    pub params: Value,
    /// Matcher expressions JavaScript must test `path` against before running middleware.
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    pub matchers: &'a [String],
}

impl<'a> ScriptRequest<'a> {
    /// `target` is the public path and query the script sees, which differs from the request for data requests.
    pub fn new(ctx: &'a Ctx, target: &str, path: &'a str) -> Self {
        let headers = ctx.headers().iter().filter_map(|(name, value)| Some((name.as_str(), value.to_str().ok()?)));
        Self {
            method: ctx.method().as_str(),
            url: format!("{}{target}", origin(ctx)),
            path,
            headers: headers.collect(),
            route: None,
            params: Value::Object(Default::default()),
            matchers: &[],
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

/// Origin of the request: forwarded headers first, then `Host`, then `[site] url`.
pub(crate) fn origin(ctx: &Ctx) -> String {
    let scheme = ctx.header("x-forwarded-proto").and_then(|p| p.split(',').next()).map(str::trim);
    let host = ctx.header("x-forwarded-host").or_else(|| ctx.header("host"));
    match host {
        Some(host) => format!("{}://{host}", scheme.unwrap_or("http")),
        None => ctx.config().site_url().to_owned(),
    }
}

/// Head of a script response: `sink.shell` of `__VOLT__.handle` and `__VOLT__.middleware`.
#[derive(Deserialize)]
struct Head {
    status: u16,
    #[serde(default)]
    headers: Vec<(String, String)>,
}

/// Collects a script response: its head, then the body chunks.
pub(crate) async fn collect(mut events: UnboundedReceiver<StreamEvent>) -> Result<Response> {
    let head: Head = match events.recv().await {
        Some(StreamEvent::Shell(head)) => serde_json::from_str(&head)?,
        Some(StreamEvent::Done(Err(message))) => return Err(Error::internal(message)),
        _ => return Err(Error::internal("the handler answered without a response")),
    };
    let mut body = BytesMut::new();
    loop {
        match events.recv().await {
            Some(StreamEvent::Chunk(chunk)) => body.extend_from_slice(&chunk),
            Some(StreamEvent::Done(Ok(_))) => break,
            Some(StreamEvent::Done(Err(message))) => return Err(Error::internal(message)),
            Some(StreamEvent::Shell(_)) | None => return Err(Error::internal("the handler response ended early")),
        }
    }
    let status =
        StatusCode::from_u16(head.status).map_err(|_| Error::internal(format!("invalid status {}", head.status)))?;
    let mut response = Response::new(status, body.freeze());
    for (name, value) in head.headers {
        // hyper frames the body itself.
        if name.eq_ignore_ascii_case("content-length") || name.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            response.headers.append(name, value);
        }
    }
    Ok(response)
}

/// What middleware decided for a request.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// Continue, to `rewrite` when set, with `headers` added to the final response and the request headers
    /// replaced by `request` when middleware overrode them.
    Continue { rewrite: Option<String>, headers: HeaderMap, request: Option<HeaderMap> },
    /// A response of its own, a redirect included.
    Respond(Response),
}

impl Outcome {
    pub fn read(mut response: Response) -> Self {
        let rewrite = take(&mut response.headers, REWRITE);
        let next = take(&mut response.headers, NEXT).is_some();
        if rewrite.is_none() && !next {
            return Self::Respond(response);
        }
        let request = take(&mut response.headers, OVERRIDE).map(|names| {
            let mut headers = HeaderMap::new();
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                let key = format!("{REQUEST_PREFIX}{name}");
                for value in response.headers.get_all(key.as_str()) {
                    if let Ok(name) = HeaderName::try_from(name) {
                        headers.append(name, value.clone());
                    }
                }
            }
            headers
        });
        let mut headers = HeaderMap::new();
        for (name, value) in &response.headers {
            if !name.as_str().starts_with(REQUEST_PREFIX) && name != header::CONTENT_TYPE {
                headers.append(name.clone(), value.clone());
            }
        }
        Self::Continue { rewrite, headers, request }
    }
}

fn take(headers: &mut HeaderMap, name: &'static str) -> Option<String> {
    headers.remove(name).and_then(|v| v.to_str().ok().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(headers: &[(&'static str, &'static str)]) -> Response {
        let mut response = Response::empty(StatusCode::OK);
        for (name, value) in headers {
            response.headers.append(*name, HeaderValue::from_static(value));
        }
        response
    }

    #[test]
    fn reads_middleware_outcomes() {
        let next = Outcome::read(response(&[
            (NEXT, "1"),
            ("x-hello", "world"),
            ("x-middleware-ran", "1"),
            (OVERRIDE, "x-user,cookie"),
            ("x-middleware-request-x-user", "ada"),
            ("x-middleware-request-cookie", "a=1"),
        ]));
        let Outcome::Continue { rewrite: None, headers, request: Some(request) } = next else { panic!("{next:?}") };
        assert_eq!(headers.get("x-hello").unwrap(), "world");
        assert_eq!(headers.get("x-middleware-ran").unwrap(), "1");
        assert_eq!(headers.len(), 2);
        assert_eq!(request.get("x-user").unwrap(), "ada");
        assert_eq!(request.get("cookie").unwrap(), "a=1");
        let rewrite = Outcome::read(response(&[(REWRITE, "http://a.test/b")]));
        assert!(
            matches!(rewrite, Outcome::Continue { rewrite: Some(url), request: None, .. } if url == "http://a.test/b")
        );
        assert!(matches!(Outcome::read(response(&[("location", "/x")])), Outcome::Respond(_)));
    }
}
