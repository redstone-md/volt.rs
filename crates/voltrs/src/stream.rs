//! Streaming response body of a page rendered with Suspense boundaries.

use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use flate2::write::GzEncoder;
use hyper::body::{Body, Frame, SizeHint};
use tokio::sync::mpsc;

use crate::document::Splice;
use crate::ssr::StreamEvent;

/// Forwards the chunks of a streaming render as they arrive, with the document additions spliced in.
/// Dropping it (the client went away) closes the channel, which aborts the render.
pub struct PageStream {
    events: mpsc::UnboundedReceiver<StreamEvent>,
    splice: Splice,
    gzip: Option<GzEncoder<Vec<u8>>>,
    path: String,
    done: bool,
}

impl PageStream {
    pub fn new(events: mpsc::UnboundedReceiver<StreamEvent>, splice: Splice, path: &str) -> Self {
        Self { events, splice, gzip: None, path: path.to_owned(), done: false }
    }

    /// Compresses the stream, flushing the encoder after every chunk so each one reaches the browser at once.
    pub fn gzip(&mut self) {
        self.gzip = Some(GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    }

    fn encode(&mut self, bytes: Vec<u8>) -> Vec<u8> {
        let Some(encoder) = self.gzip.as_mut() else {
            return bytes;
        };
        if !bytes.is_empty() && (encoder.write_all(&bytes).is_err() || encoder.flush().is_err()) {
            encoder.get_mut().clear();
        }
        let mut out = std::mem::take(encoder.get_mut());
        if self.done
            && let Some(encoder) = self.gzip.take()
            && let Ok(rest) = encoder.finish()
        {
            out.extend(rest);
        }
        out
    }
}

impl Body for PageStream {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        let mut out = Vec::new();
        // Everything already queued goes out as one frame: React flushes a boundary in several small chunks.
        loop {
            match self.events.poll_recv(cx) {
                Poll::Ready(Some(StreamEvent::Chunk(chunk))) => out.extend(self.splice.push(&chunk)),
                Poll::Ready(Some(StreamEvent::Shell(_))) => {}
                Poll::Ready(event) => {
                    if let Some(StreamEvent::Done(Err(message))) = event {
                        crate::log::error(&format!("streaming {} failed: {message}", self.path));
                    }
                    out.extend(self.splice.finish());
                    self.done = true;
                    break;
                }
                Poll::Pending => break,
            }
        }
        let out = self.encode(out);
        match (out.is_empty(), self.done) {
            (false, _) => Poll::Ready(Some(Ok(Frame::data(Bytes::from(out))))),
            (true, true) => Poll::Ready(None),
            (true, false) => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}
