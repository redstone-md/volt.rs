//! The HTTP client behind `fetch` in server code, with the `[server] fetch_allow` policy.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use http_body_util::{BodyExt, Full, Limited};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::config::ServerConfig;

/// Largest response body a server-side `fetch` reads.
const MAX_BODY: usize = 32 * 1024 * 1024;
const MAX_REDIRECTS: usize = 20;

/// Which hosts server code may fetch from, and for how long a request may take.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// `[server] fetch_allow`: empty allows every host that resolves to public addresses only.
    pub allow: Vec<String>,
    pub timeout: Duration,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self { allow: Vec::new(), timeout: Duration::from_secs(10) }
    }
}

impl FetchOptions {
    pub fn from_config(server: &ServerConfig) -> Self {
        Self { allow: server.fetch_allow.clone(), timeout: Duration::from_secs(server.fetch_timeout_secs.max(1)) }
    }

    /// `None` when the host is not allowed; `Some(true)` when it was listed, which also allows it to resolve
    /// to loopback or private addresses.
    fn permits(&self, host: &str, port: u16) -> Option<bool> {
        if self.allow.is_empty() {
            return Some(false);
        }
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        let listed = self.allow.iter().any(|entry| {
            let entry = entry.trim().to_ascii_lowercase();
            let (name, wanted) = match entry.rsplit_once(':') {
                Some((name, p)) if !name.contains(':') || name.ends_with(']') => (name.to_owned(), p.parse().ok()),
                _ => (entry.clone(), None),
            };
            let name = name.trim_start_matches('[').trim_end_matches(']');
            let host_matches = match name.strip_prefix("*.") {
                _ if name == "*" => true,
                Some(suffix) => host.ends_with(&format!(".{suffix}")),
                None => host == name,
            };
            host_matches && wanted.is_none_or(|p: u16| p == port)
        });
        listed.then_some(true)
    }
}

/// A request made by `fetch` in QuickJS.
#[derive(Debug, Deserialize)]
pub struct FetchRequest {
    pub url: String,
    #[serde(default = "get")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// `follow` (the default), `manual` or `error`.
    #[serde(default)]
    pub redirect: String,
}

fn get() -> String {
    "GET".into()
}

/// The head of a response, handed to the JavaScript `Response`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirected: bool,
}

/// Sends requests for server code on a small runtime of its own, so callers on plain threads can use it.
#[derive(Debug, Clone)]
pub struct Fetcher {
    options: Arc<FetchOptions>,
}

impl Fetcher {
    pub fn new(options: FetchOptions) -> Self {
        Self { options: Arc::new(options) }
    }

    /// Starts `request` and calls `done` with the result from a fetch thread.
    pub fn spawn(
        &self,
        request: FetchRequest,
        body: Bytes,
        done: impl FnOnce(Result<(FetchResponse, Bytes), String>) + Send + 'static,
    ) {
        let fetcher = self.clone();
        runtime().spawn(async move { done(fetcher.send(request, body).await) });
    }

    /// Sends `request`, following redirects unless asked not to, within the configured timeout.
    pub async fn send(&self, request: FetchRequest, body: Bytes) -> Result<(FetchResponse, Bytes), String> {
        let url = request.url.clone();
        match tokio::time::timeout(self.options.timeout, self.follow(request, body)).await {
            Ok(result) => result.map_err(|e| format!("fetch {url} failed: {e}")),
            Err(_) => Err(format!("fetch {url} timed out after {}s", self.options.timeout.as_secs())),
        }
    }

    async fn follow(&self, request: FetchRequest, mut body: Bytes) -> Result<(FetchResponse, Bytes), String> {
        let mut method =
            Method::from_bytes(request.method.to_ascii_uppercase().as_bytes()).map_err(|e| e.to_string())?;
        let mut uri: Uri = request.url.parse().map_err(|_| format!("invalid URL `{}`", request.url))?;
        let mut headers = request.headers;
        for hop in 0..=MAX_REDIRECTS {
            let response = self.once(&method, &uri, &headers, body.clone()).await?;
            let status = response.status();
            let location = response.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).map(str::to_owned);
            let Some(location) = location.filter(|_| status.is_redirection() && request.redirect != "manual") else {
                return read(response, &uri, hop > 0).await;
            };
            if request.redirect == "error" {
                return Err(format!("redirected to {location} with redirect: \"error\""));
            }
            uri = resolve(&uri, &location)?;
            let rewrites_to_get = status == StatusCode::SEE_OTHER
                || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND) && method == Method::POST);
            if rewrites_to_get && method != Method::HEAD {
                method = Method::GET;
                body = Bytes::new();
                headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
            }
        }
        Err(format!("more than {MAX_REDIRECTS} redirects"))
    }

    /// One request on a fresh connection.
    async fn once(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &[(String, String)],
        body: Bytes,
    ) -> Result<hyper::Response<hyper::body::Incoming>, String> {
        let https = match uri.scheme_str() {
            Some("https") => true,
            Some("http") => false,
            _ => return Err(format!("unsupported URL scheme in `{uri}`")),
        };
        let host = uri.host().ok_or_else(|| format!("`{uri}` has no host"))?.to_owned();
        let port = uri.port_u16().unwrap_or(if https { 443 } else { 80 });
        let trusted = self
            .options
            .permits(&host, port)
            .ok_or_else(|| format!("host `{host}` is not listed in [server] fetch_allow"))?;
        let stream = connect(&host, port, trusted).await?;
        let mut request = hyper::Request::builder()
            .method(method.clone())
            .uri(uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"))
            .body(Full::new(body))
            .map_err(|e| e.to_string())?;
        let target = request.headers_mut();
        for (name, value) in headers {
            if let (Ok(name), Ok(value)) = (HeaderName::try_from(name.as_str()), HeaderValue::try_from(value.as_str()))
            {
                target.append(name, value);
            }
        }
        let authority = uri.authority().map(|a| a.as_str()).unwrap_or(&host);
        target.insert(header::HOST, HeaderValue::try_from(authority).map_err(|e| e.to_string())?);
        target.entry(header::USER_AGENT).or_insert(HeaderValue::from_static("volt"));
        target.entry(header::ACCEPT).or_insert(HeaderValue::from_static("*/*"));
        if https {
            let name = rustls::pki_types::ServerName::try_from(host.trim_start_matches('[').trim_end_matches(']'))
                .map_err(|e| e.to_string())?
                .to_owned();
            let tls = tokio_rustls::TlsConnector::from(tls_config()?).connect(name, stream).await;
            exchange(tls.map_err(|e| e.to_string())?, request).await
        } else {
            exchange(stream, request).await
        }
    }
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    io: S,
    request: hyper::Request<Full<Bytes>>,
) -> Result<hyper::Response<hyper::body::Incoming>, String> {
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(io)).await.map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender.send_request(request).await.map_err(|e| e.to_string())
}

async fn read(
    response: hyper::Response<hyper::body::Incoming>,
    uri: &Uri,
    redirected: bool,
) -> Result<(FetchResponse, Bytes), String> {
    let (parts, body) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
        .collect();
    let body = Limited::new(body, MAX_BODY)
        .collect()
        .await
        .map_err(|e| format!("cannot read the response body: {e}"))?
        .to_bytes();
    let head = FetchResponse {
        status: parts.status.as_u16(),
        status_text: parts.status.canonical_reason().unwrap_or_default().to_owned(),
        headers,
        url: uri.to_string(),
        redirected,
    };
    Ok((head, body))
}

/// Connects to the first reachable address of `host`. Unless the host was listed explicitly, every address
/// must be public, so that code fetching user-supplied URLs cannot reach the machine's own network.
async fn connect(host: &str, port: u16, trusted: bool) -> Result<TcpStream, String> {
    let name = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<SocketAddr> =
        tokio::net::lookup_host((name, port)).await.map_err(|e| format!("cannot resolve `{host}`: {e}"))?.collect();
    if !trusted && let Some(private) = addresses.iter().find(|a| !is_public(a.ip())) {
        return Err(format!(
            "`{host}` resolves to the non-public address {}; list it in [server] fetch_allow to allow it",
            private.ip()
        ));
    }
    let mut last = format!("`{host}` has no address");
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(error) => last = format!("cannot connect to {address}: {error}"),
        }
    }
    Err(last)
}

/// Whether an address is on the public internet: not loopback, private, link-local, shared or reserved.
pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// `location` resolved against the URL it was returned for.
fn resolve(base: &Uri, location: &str) -> Result<Uri, String> {
    if location.contains("://") {
        return location.parse().map_err(|_| format!("invalid redirect location `{location}`"));
    }
    let scheme = base.scheme_str().unwrap_or("http");
    let authority = base.authority().map(|a| a.as_str()).unwrap_or_default();
    let target = if let Some(rest) = location.strip_prefix("//") {
        format!("{scheme}://{rest}")
    } else if location.starts_with('/') {
        format!("{scheme}://{authority}{location}")
    } else {
        let dir = base.path().rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{scheme}://{authority}{dir}/{location}")
    };
    target.parse().map_err(|_| format!("invalid redirect location `{location}`"))
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            use rustls_platform_verifier::BuilderVerifierExt;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .and_then(|builder| builder.with_platform_verifier())
                .map_err(|e| format!("cannot set up TLS: {e}"))?
                .with_no_client_auth();
            Ok(Arc::new(config))
        })
        .clone()
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("volt-fetch")
            .enable_all()
            .build()
            .expect("cannot start the fetch runtime")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_the_allow_list() {
        let open = FetchOptions::default();
        assert_eq!(open.permits("example.com", 443), Some(false));
        let listed = FetchOptions { allow: vec!["*.example.com".into(), "localhost:4000".into()], ..open };
        assert_eq!(listed.permits("api.example.com", 443), Some(true));
        assert_eq!(listed.permits("example.com", 443), None);
        assert_eq!(listed.permits("localhost", 4000), Some(true));
        assert_eq!(listed.permits("localhost", 4001), None);
        let any = FetchOptions { allow: vec!["*".into()], ..FetchOptions::default() };
        assert_eq!(any.permits("10.0.0.1", 80), Some(true));
    }

    #[test]
    fn recognizes_public_addresses() {
        for private in ["127.0.0.1", "10.1.2.3", "192.168.0.1", "169.254.169.254", "100.64.0.1", "::1", "fd00::1"] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "2606:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn resolves_redirects() {
        let base: Uri = "https://a.test/x/y?q".parse().unwrap();
        assert_eq!(resolve(&base, "/z").unwrap().to_string(), "https://a.test/z");
        assert_eq!(resolve(&base, "z").unwrap().to_string(), "https://a.test/x/z");
        assert_eq!(resolve(&base, "http://b.test/").unwrap().to_string(), "http://b.test/");
    }
}
