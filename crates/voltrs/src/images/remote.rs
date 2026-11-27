//! Fetching of allowed remote images for the optimizer.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderValue, StatusCode, header};
use http_body_util::{BodyExt, Empty, Limited};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::connect::dns::Name;
use hyper_util::rt::TokioExecutor;
use tokio::sync::Semaphore;

use crate::config::{ImageConfig, RemoteUrl};
use crate::error::{Error, Result};

/// Largest remote original accepted.
pub(super) const MAX_BYTES: usize = 20 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Budget for a whole download, redirects included, from the first connection until the last body has arrived.
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 3;
/// Downloads in flight at once, so distinct URLs cannot each buffer `MAX_BYTES` at the same time.
const MAX_DOWNLOADS: usize = 8;
/// Only formats the optimizer decodes: origins that negotiate (`f_auto`, `auto=format`) would otherwise answer
/// AVIF, which is served unresized and only to clients that accept it.
const ACCEPT: &str = "image/webp,image/png,image/jpeg,image/gif,image/*;q=0.5";

/// HTTP(S) client that only reaches hosts allowed by `[images] remote_patterns`.
pub(super) struct Remote {
    client: Client<HttpsConnector<HttpConnector<Resolver>>, Empty<Bytes>>,
    allow_local_ip: bool,
    downloads: Semaphore,
}

impl Remote {
    pub(super) fn new(allow_local_ip: bool) -> Result<Self> {
        let mut http = HttpConnector::new_with_resolver(Resolver { allow_local_ip });
        http.enforce_http(false);
        http.set_connect_timeout(Some(CONNECT_TIMEOUT));
        let https = HttpsConnectorBuilder::new()
            .with_provider_and_webpki_roots(Arc::new(rustls::crypto::ring::default_provider()))
            .map_err(|e| Error::internal(e.to_string()))?
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(http);
        let client = Client::builder(TokioExecutor::new()).pool_idle_timeout(Duration::from_secs(30)).build(https);
        Ok(Self { client, allow_local_ip, downloads: Semaphore::new(MAX_DOWNLOADS) })
    }

    /// Downloads an image, following redirects only to URLs that are allowed as well.
    pub(super) async fn fetch(&self, url: &RemoteUrl, config: &ImageConfig) -> Result<Vec<u8>> {
        let _permit = self.downloads.acquire().await.map_err(|_| Error::internal("download pool closed"))?;
        tokio::time::timeout(TIMEOUT, self.follow(url, config))
            .await
            .unwrap_or_else(|_| Err(gateway(format!("timed out fetching {}", url.href()))))
    }

    async fn follow(&self, url: &RemoteUrl, config: &ImageConfig) -> Result<Vec<u8>> {
        let mut current = url.clone();
        for _ in 0..=MAX_REDIRECTS {
            match self.get(&current).await {
                Ok(Fetched::Body(body)) => return Ok(body),
                Ok(Fetched::Redirect(location)) => {
                    let next = resolve(&current, &location)
                        .ok_or_else(|| gateway(format!("invalid redirect from {}", current.href())))?;
                    if !config.allows_remote(&next) {
                        return Err(Error::bad_request(format!(
                            "{} redirects to {}, which images.remote_patterns does not allow",
                            current.href(),
                            next.href()
                        )));
                    }
                    current = next;
                }
                Err(error) => return Err(error),
            }
        }
        Err(gateway(format!("too many redirects fetching {}", url.href())))
    }

    async fn get(&self, url: &RemoteUrl) -> Result<Fetched> {
        if !self.allow_local_ip && url.hostname.parse::<IpAddr>().is_ok_and(is_local) {
            return Err(Error::bad_request(format!("{} is a local address", url.hostname)));
        }
        let uri: http::Uri = url.href().parse().map_err(|_| Error::bad_request("invalid remote URL"))?;
        let request = http::Request::get(uri)
            .header(header::USER_AGENT, HeaderValue::from_static("volt-image-optimizer"))
            .header(header::ACCEPT, HeaderValue::from_static(ACCEPT))
            .body(Empty::new())
            .map_err(|e| Error::internal(e.to_string()))?;
        let response = self.client.request(request).await.map_err(|e| gateway(format!("{}: {e}", url.href())))?;
        let status = response.status();
        if status.is_redirection() {
            let location = response.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
            return Ok(Fetched::Redirect(location.to_owned()));
        }
        if !status.is_success() {
            let code = if status == StatusCode::NOT_FOUND { StatusCode::NOT_FOUND } else { StatusCode::BAD_GATEWAY };
            return Err(Error::new(code, format!("{} answered {status}", url.href())));
        }
        let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !content_type.trim().to_ascii_lowercase().starts_with("image/") {
            return Err(Error::bad_request(format!("{} is not an image (`{content_type}`)", url.href())));
        }
        let declared = response.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse().ok());
        if declared.is_some_and(|length: usize| length > MAX_BYTES) {
            return Err(too_large(url));
        }
        let body = Limited::new(response.into_body(), MAX_BYTES).collect().await.map_err(|error| {
            if error.is::<http_body_util::LengthLimitError>() {
                too_large(url)
            } else {
                gateway(format!("{}: {error}", url.href()))
            }
        })?;
        Ok(Fetched::Body(body.to_bytes().to_vec()))
    }
}

enum Fetched {
    Body(Vec<u8>),
    Redirect(String),
}

fn gateway(message: String) -> Error {
    Error::new(StatusCode::BAD_GATEWAY, message)
}

fn too_large(url: &RemoteUrl) -> Error {
    Error::new(StatusCode::PAYLOAD_TOO_LARGE, format!("{} is larger than {} MB", url.href(), MAX_BYTES >> 20))
}

/// Target of a `Location` header: absolute, protocol-relative or a path on the same origin.
fn resolve(base: &RemoteUrl, location: &str) -> Option<RemoteUrl> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return RemoteUrl::parse(location);
    }
    if let Some(rest) = location.strip_prefix("//") {
        return RemoteUrl::parse(&format!("{}://{rest}", base.protocol));
    }
    if location.starts_with('/') {
        let origin = RemoteUrl { pathname: String::new(), search: String::new(), ..base.clone() };
        return RemoteUrl::parse(&format!("{}{location}", origin.href()));
    }
    None
}

/// Loopback, private, link-local and other addresses that must not be reached through a public URL.
fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (18..20).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// DNS resolution that refuses local addresses. Checking the resolved addresses (rather than the host name)
/// also covers names that point at internal hosts and DNS rebinding, since the connection uses these addresses.
#[derive(Clone)]
struct Resolver {
    allow_local_ip: bool,
}

impl tower_service::Service<Name> for Resolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = std::io::Result<Self::Response>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let allow_local_ip = self.allow_local_ip;
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if !allow_local_ip && addresses.iter().any(|a| is_local(a.ip())) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("{name} resolves to a local address"),
                ));
            }
            Ok(addresses.into_iter())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_local_addresses() {
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "100.64.0.1",
        ] {
            assert!(is_local(local.parse().unwrap()), "{local}");
        }
        for public in ["93.184.216.34", "2606:2800:220:1::1", "8.8.8.8"] {
            assert!(!is_local(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn resolves_redirect_targets() {
        let base = RemoteUrl::parse("https://cdn.example.com:8443/a/b.png?x").unwrap();
        assert_eq!(resolve(&base, "/c.png").unwrap().href(), "https://cdn.example.com:8443/c.png");
        assert_eq!(resolve(&base, "//other.dev/d.png").unwrap().href(), "https://other.dev/d.png");
        assert_eq!(resolve(&base, "http://x.dev/e").unwrap().href(), "http://x.dev/e");
        assert!(resolve(&base, "relative.png").is_none());
    }
}
