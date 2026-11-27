//! Development live reload over Server-Sent Events.

use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use tokio::sync::mpsc;

/// Browser-side client injected into dev pages; `{base}` is replaced with the base path.
const CLIENT: &str = r#"<script type="module">
const connect = (stale) => {
  const source = new EventSource("{base}/_volt/hmr");
  source.onopen = () => { if (stale) location.reload(); };
  source.addEventListener("reload", () => location.reload());
  source.addEventListener("css", () => {
    for (const link of document.querySelectorAll('link[rel="stylesheet"][href^="{base}/_volt/"]')) {
      const url = new URL(link.href);
      url.searchParams.set("t", Date.now());
      link.href = url.toString();
    }
  });
  source.addEventListener("error", (event) => {
    if (event.data) console.error("[volt]", event.data);
  });
  source.onerror = () => { source.close(); setTimeout(() => connect(true), 400); };
};
connect(false);
</script>"#;

/// The live reload client for an app mounted at `base_path`.
pub fn client(base_path: &str) -> String {
    CLIENT.replace("{base}", base_path)
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
