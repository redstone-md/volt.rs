//! Development live reload and Fast Refresh updates over Server-Sent Events.

use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use serde_json::json;
use tokio::sync::mpsc;

use crate::manifest::Manifest;

/// Browser-side client injected into dev pages; `{base}` is replaced with the base path and `{revision}` with
/// the revision of the build the page was rendered from. It applies `update` events through the Fast Refresh
/// runtime of the page (`window.__VOLT_REFRESH__`, absent on error pages, which reload instead), swaps the
/// stylesheets and shows build and runtime errors in an overlay that keeps the page and its state underneath.
const CLIENT: &str = r#"<script type="module">
const overlay = {
  host: null,
  show(title, detail) {
    if (!this.host) {
      this.host = document.createElement("volt-dev-overlay");
      const root = this.host.attachShadow({ mode: "open" });
      root.innerHTML = `<style>
:host{position:fixed;inset:0;z-index:2147483647;display:flex;align-items:flex-start;justify-content:center;background:rgba(9,9,11,.6);padding:48px 16px;overflow:auto;font:14px/1.6 ui-monospace,monospace}
section{box-sizing:border-box;width:100%;max-width:960px;background:#0b0b0f;color:#e4e4e7;border:1px solid #27272a;border-radius:12px;padding:24px}
header{display:flex;justify-content:space-between;gap:16px}h1{margin:0;color:#facc15;font-size:16px}
button{background:none;border:0;color:#a1a1aa;font:inherit;font-size:20px;line-height:1;cursor:pointer}
pre{margin:16px 0 0;white-space:pre-wrap;background:#18181b;border:1px solid #27272a;border-radius:8px;padding:16px;overflow:auto}</style>
<section role="alert"><header><h1></h1><button type="button" aria-label="Close">×</button></header><pre></pre></section>`;
      root.querySelector("button").onclick = () => this.hide();
      // React empties <body> when the root fails or remounts; the overlay outlives it while it is shown.
      new MutationObserver(() => this.visible && this.attach()).observe(document, { childList: true, subtree: true });
    }
    const root = this.host.shadowRoot;
    root.querySelector("h1").textContent = "⚡ " + title;
    root.querySelector("pre").textContent = detail;
    this.visible = true;
    this.attach();
  },
  attach() {
    if (!this.host.isConnected) (document.body ?? document.documentElement).appendChild(this.host);
  },
  hide() {
    this.visible = false;
    this.host?.remove();
  },
};
const describe = (error) => (error && error.stack) || String(error);
addEventListener("error", (event) => overlay.show("Unhandled runtime error", describe(event.error ?? event.message)));
addEventListener("unhandledrejection", (event) => overlay.show("Unhandled runtime error", describe(event.reason)));
addEventListener("volt:error", (event) => overlay.show("Runtime error", describe(event.detail)));

const swapStyles = (hrefs) => {
  const links = [...document.querySelectorAll('link[rel="stylesheet"]')].filter((link) =>
    new URL(link.href, location.href).pathname.startsWith("{base}/_volt/static/css/"),
  );
  const current = links.map((link) => new URL(link.href, location.href).pathname);
  if (current.length === hrefs.length && current.every((href, i) => href === hrefs[i])) return;
  const anchor = links[0] ?? null;
  for (const href of hrefs) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = href;
    if (anchor) anchor.before(link);
    else document.head.append(link);
  }
  for (const link of links) link.remove();
};

let revision = {revision};
let queue = Promise.resolve();
const apply = async (update) => {
  const refresh = window.__VOLT_REFRESH__;
  if (!refresh || update.base > revision) return location.reload();
  swapStyles(update.css);
  // Hidden first: render errors of the refreshed components report themselves once React re-renders.
  overlay.hide();
  try {
    if (!(await refresh.apply(update, revision))) return location.reload();
    revision = update.revision;
  } catch (error) {
    overlay.show("Runtime error", describe(error));
  }
};

const connect = (stale) => {
  const source = new EventSource("{base}/_volt/hmr");
  source.onopen = () => { if (stale) location.reload(); };
  source.addEventListener("reload", () => location.reload());
  source.addEventListener("update", (event) => {
    const update = JSON.parse(event.data);
    queue = queue.then(() => apply(update));
  });
  source.addEventListener("failed", (event) => overlay.show("Build failed", event.data));
  source.onerror = () => { source.close(); setTimeout(() => connect(true), 400); };
};
connect(false);
</script>"#;

/// The live reload client for an app mounted at `base_path`, on a page rendered from build `revision`.
pub fn client(base_path: &str, revision: u64) -> String {
    CLIENT.replace("{base}", base_path).replace("{revision}", &revision.to_string())
}

/// The event that brings browsers showing build `previous` to build `next`, with its data: `failed` with the
/// error of a failed rebuild, `update` with the modules and stylesheets to swap in place, or `reload`.
pub fn event(previous: &Manifest, next: &Manifest) -> (&'static str, String) {
    if let Some(error) = &next.error {
        return ("failed", error.clone());
    }
    match &next.hot {
        Some(hot) if hot.base <= previous.revision && previous.revision < next.revision => {
            let data = json!({
                "base": hot.base,
                "modules": hot.modules,
                "importers": hot.importers,
                "css": next.client.css,
                "revision": next.revision,
            });
            ("update", data.to_string())
        }
        _ => ("reload", String::new()),
    }
}

#[derive(Default)]
pub struct Hub {
    clients: Mutex<Vec<mpsc::UnboundedSender<Bytes>>>,
}

impl Hub {
    pub fn subscribe(&self) -> EventStream {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(Bytes::from_static(b": connected\n\n"));
        if let Ok(mut clients) = self.clients.lock() {
            clients.push(tx);
        }
        EventStream { rx }
    }

    pub fn broadcast(&self, event: &str, data: &str) {
        let data = data.replace('\n', "\ndata: ");
        let frame = Bytes::from(format!("event: {event}\ndata: {data}\n\n"));
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain(|client| client.send(frame.clone()).is_ok());
        }
    }
}

/// Streaming response body for SSE clients.
pub struct EventStream {
    rx: mpsc::UnboundedReceiver<Bytes>,
}

impl Body for EventStream {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        self.rx.poll_recv(cx).map(|chunk| chunk.map(|bytes| Ok(Frame::data(bytes))))
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{HotModule, HotUpdate};

    #[test]
    fn fills_in_the_client() {
        let script = client("/docs", 7);
        assert!(script.contains("new EventSource(\"/docs/_volt/hmr\")"));
        assert!(script.contains("let revision = 7;"));
    }

    #[test]
    fn picks_events() {
        let previous = Manifest { revision: 3, ..Manifest::default() };
        let url = "/_volt/static/js/app/page.js";
        let update = HotUpdate {
            base: 3,
            modules: vec![HotModule { url: url.into(), revision: 4 }],
            importers: [(url.to_owned(), vec!["/_volt/static/js/_virtual/home.js".to_owned()])].into(),
        };
        let mut next = Manifest { revision: 4, hot: Some(update), ..Manifest::default() };
        next.client.css = vec!["/_volt/static/css/app-1.css".into()];
        let (name, data) = event(&previous, &next);
        assert_eq!(name, "update");
        let data: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(data["modules"][0]["url"], url);
        assert_eq!(data["modules"][0]["revision"], 4);
        assert_eq!(data["importers"][url][0], "/_volt/static/js/_virtual/home.js");
        assert_eq!(data["base"], 3);
        assert_eq!(data["css"][0], "/_volt/static/css/app-1.css");
        assert_eq!(data["revision"], 4);

        let earlier = Manifest { revision: 2, ..Manifest::default() };
        assert_eq!(event(&earlier, &next).0, "reload");
        next.hot.as_mut().unwrap().base = 1;
        assert_eq!(event(&earlier, &next).0, "update");
        assert_eq!(event(&next.clone(), &next).0, "reload");
        next.hot = None;
        assert_eq!(event(&previous, &next).0, "reload");
        next.error = Some("boom".into());
        assert_eq!(event(&previous, &next), ("failed", "boom".to_owned()));
    }
}
