//! React rendering on a pool of QuickJS workers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bytes::Bytes;
use rquickjs::{
    CaughtError, Context, Ctx, Function, Object, Promise, Runtime, TypedArray, Value, promise::PromiseState,
};
use tokio::sync::{mpsc, oneshot};

use crate::error::{Error, Result};

const POLYFILLS: &str = include_str!("polyfills.js");
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an aborted render may take to flush its client-rendered boundaries before it is failed.
const ABORT_GRACE: Duration = Duration::from_secs(2);
const GC_THRESHOLD: usize = 8 * 1024 * 1024;
/// Renders one worker interleaves while they wait on timers. It is kept small because all their work
/// after the timers runs on this one thread: further jobs stay in the shared queue for idler workers.
const MAX_ACTIVE: usize = 8;

struct Job {
    method: &'static str,
    payload: String,
    reply: Reply,
}

/// Where the result of a job goes.
enum Reply {
    Call(oneshot::Sender<Result<String, String>>),
    Stream(Arc<Sink>),
}

impl Reply {
    fn finish(self, result: Result<String, String>) {
        match self {
            Self::Call(reply) => {
                let _ = reply.send(result);
            }
            Self::Stream(sink) => sink.close(result),
        }
    }
}

/// Output of a streamed render, in order: at most one shell, its chunks, then the end.
#[derive(Debug)]
pub enum StreamEvent {
    /// JSON sent once the shell is ready (status, route), before any chunk.
    Shell(String),
    Chunk(Bytes),
    /// The function's return value: empty after a streamed shell, else a complete buffered result.
    Done(Result<String, String>),
}

/// The channel a streaming render writes to. Writes fail once the receiver is gone or the job has ended,
/// which tells the JavaScript side to abort rendering. The render may set `sink.abort(reason)`, which the
/// worker calls when the render runs past its deadline or waits on a promise nothing can resolve.
struct Sink {
    events: Mutex<Option<mpsc::UnboundedSender<StreamEvent>>>,
}

impl Sink {
    fn send(&self, event: StreamEvent) -> bool {
        match self.events.lock().ok().as_deref() {
            Some(Some(events)) => events.send(event).is_ok(),
            _ => false,
        }
    }

    fn close(&self, result: Result<String, String>) {
        if let Some(events) = self.events.lock().ok().and_then(|mut e| e.take()) {
            let _ = events.send(StreamEvent::Done(result));
        }
    }
}

/// Configuration of the rendering pool.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub bundle: PathBuf,
    pub workers: usize,
    pub memory_limit: usize,
    pub dev: bool,
}

/// A pool of isolated QuickJS runtimes executing the server bundle.
#[derive(Clone)]
pub struct Engine {
    jobs: flume::Sender<Job>,
    shared: Arc<Shared>,
}

/// Bundle location and its version, swapped by [`Engine::reload`].
struct Shared {
    generation: AtomicU64,
    bundle: std::sync::RwLock<PathBuf>,
}

impl Shared {
    fn bundle(&self) -> PathBuf {
        self.bundle.read().map(|b| b.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

impl Engine {
    /// Starts the workers and waits until the first one has evaluated the bundle.
    pub fn start(options: EngineOptions) -> Result<Self> {
        let (jobs, receiver) = flume::unbounded::<Job>();
        let shared =
            Arc::new(Shared { generation: AtomicU64::new(0), bundle: std::sync::RwLock::new(options.bundle.clone()) });
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        for index in 0..options.workers.max(1) {
            let receiver = receiver.clone();
            let shared = shared.clone();
            let options = options.clone();
            let ready = ready_tx.clone();
            thread::Builder::new()
                .name(format!("volt-ssr-{index}"))
                .stack_size(16 * 1024 * 1024)
                .spawn(move || Worker::run(options, receiver, shared, ready))
                .map_err(|e| Error::internal(format!("cannot spawn SSR worker: {e}")))?;
        }
        drop(ready_tx);
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { jobs, shared }),
            Ok(Err(message)) => Err(Error::internal(message)),
            Err(_) => Err(Error::internal("SSR workers exited during startup")),
        }
    }

    /// Calls `globalThis.__VOLT__[method](payload)` and returns its string result.
    pub async fn call(&self, method: &'static str, payload: String) -> Result<String> {
        let (reply, rx) = oneshot::channel();
        self.send(Job { method, payload, reply: Reply::Call(reply) })?;
        rx.await.map_err(|_| Error::internal("SSR worker dropped the request"))?.map_err(Error::internal)
    }

    /// Calls `globalThis.__VOLT__[method](payload, sink)`, where `sink.shell(json)` and `sink.write(bytes)`
    /// forward output as it is produced. Both return `false` once the receiver is dropped.
    pub fn stream(&self, method: &'static str, payload: String) -> Result<mpsc::UnboundedReceiver<StreamEvent>> {
        let (events, rx) = mpsc::unbounded_channel();
        let sink = Arc::new(Sink { events: Mutex::new(Some(events)) });
        self.send(Job { method, payload, reply: Reply::Stream(sink) })?;
        Ok(rx)
    }

    fn send(&self, job: Job) -> Result<()> {
        self.jobs.send(job).map_err(|_| Error::internal("SSR pool is not running"))
    }

    /// Makes every worker evaluate `bundle` before its next job.
    pub fn reload(&self, bundle: PathBuf) {
        if let Ok(mut current) = self.shared.bundle.write() {
            *current = bundle;
        }
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// A job whose function returned a pending promise.
struct Active<'js> {
    promise: Promise<'js>,
    reply: Reply,
    /// The sink object of a streaming job, whose `abort` ends the render early.
    sink: Option<Object<'js>>,
    deadline: Instant,
    aborted: bool,
}

impl Active<'_> {
    /// Asks the render to finish now; returns `false` when it cannot be asked (no `abort`, or asked already).
    fn abort(&mut self, reason: &str) -> bool {
        if self.aborted {
            return false;
        }
        let Some(abort) = self.sink.as_ref().and_then(|sink| sink.get::<_, Function>("abort").ok()) else {
            return false;
        };
        self.aborted = true;
        self.deadline = Instant::now() + ABORT_GRACE;
        if let Err(error) = abort.call::<_, ()>((reason,)) {
            crate::log::js("error", &describe(abort.ctx(), error));
        }
        true
    }
}

struct Worker {
    /// Owns the QuickJS heap the context lives in.
    _runtime: Runtime,
    context: Context,
    generation: u64,
    deadline: Arc<Mutex<Option<Instant>>>,
}

impl Worker {
    fn run(
        options: EngineOptions,
        jobs: flume::Receiver<Job>,
        shared: Arc<Shared>,
        ready: std::sync::mpsc::Sender<Result<(), String>>,
    ) {
        let mut worker = match Self::load(&options, &shared.bundle(), shared.generation()) {
            Ok(worker) => {
                let _ = ready.send(Ok(()));
                worker
            }
            Err(message) => {
                let _ = ready.send(Err(message));
                return;
            }
        };
        while let Ok(job) = jobs.recv() {
            let current = shared.generation();
            if current != worker.generation {
                match Self::load(&options, &shared.bundle(), current) {
                    Ok(fresh) => worker = fresh,
                    Err(message) => {
                        job.reply.finish(Err(message));
                        continue;
                    }
                }
            }
            worker.serve(job, &jobs, &shared);
        }
    }

    fn load(options: &EngineOptions, bundle: &Path, generation: u64) -> Result<Self, String> {
        let code = std::fs::read_to_string(bundle)
            .map_err(|e| format!("cannot read server bundle {}: {e}", bundle.display()))?;
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(options.memory_limit);
        runtime.set_gc_threshold(GC_THRESHOLD);
        runtime.set_max_stack_size(8 * 1024 * 1024);
        let deadline: Arc<Mutex<Option<Instant>>> = Arc::default();
        let watched = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            watched.lock().ok().and_then(|d| *d).is_some_and(|d| Instant::now() > d)
        })));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let env = if options.dev { "development" } else { "production" };
        context.with(|ctx| -> Result<(), String> {
            install_host(&ctx, env).map_err(|e| describe(&ctx, e))?;
            ctx.eval::<(), _>(POLYFILLS).map_err(|e| describe(&ctx, e))?;
            eval_file(&ctx, &code, bundle).map_err(|e| describe(&ctx, e))
        })?;
        Ok(Self { _runtime: runtime, context, generation, deadline })
    }

    /// Runs `first` and every job taken while it waits on timers, until all of them have settled.
    /// Renders interleave on the one JavaScript event loop, so a Suspense boundary waiting on a slow
    /// promise does not hold the worker.
    fn serve(&self, first: Job, jobs: &flume::Receiver<Job>, shared: &Shared) {
        self.context.with(|ctx| {
            let mut active = Vec::new();
            active.extend(self.start(&ctx, first));
            let tick = match ctx.globals().get::<_, Function>("__volt_tick") {
                Ok(tick) => tick,
                Err(error) => {
                    let message = describe(&ctx, error);
                    for job in active {
                        job.reply.finish(Err(message.clone()));
                    }
                    return;
                }
            };
            while !active.is_empty() {
                self.guarded(|| while ctx.execute_pending_job() {});
                active = self.settle(&ctx, active);
                if active.is_empty() {
                    break;
                }
                let wait = self.guarded(|| tick.call::<_, i32>(()));
                let wait = match wait {
                    Ok(0) => continue,
                    Ok(wait) => wait,
                    Err(error) => {
                        crate::log::js("error", &describe(&ctx, error));
                        continue;
                    }
                };
                if wait < 0 {
                    let reason = "the render awaits a promise nothing resolves";
                    let mut asked = false;
                    for job in &mut active {
                        asked |= self.guarded(|| job.abort(reason));
                    }
                    if asked {
                        continue;
                    }
                    for job in active.drain(..) {
                        job.reply.finish(Err(format!("render never settled: {reason}")));
                    }
                    break;
                }
                let until_deadline = active
                    .iter()
                    .map(|job| job.deadline.saturating_duration_since(Instant::now()))
                    .min()
                    .unwrap_or(RENDER_TIMEOUT);
                let timeout = Duration::from_millis(wait as u64).min(until_deadline).max(Duration::from_millis(1));
                let accepting = active.len() < MAX_ACTIVE && shared.generation() == self.generation;
                match accepting.then(|| jobs.recv_timeout(timeout)) {
                    Some(Ok(job)) => active.extend(self.start(&ctx, job)),
                    Some(Err(flume::RecvTimeoutError::Timeout)) => {}
                    _ => thread::sleep(timeout),
                }
            }
        });
    }

    /// Calls the job's function; a non-promise result or a synchronous error finishes it at once.
    fn start<'js>(&self, ctx: &Ctx<'js>, job: Job) -> Option<Active<'js>> {
        let deadline = Instant::now() + RENDER_TIMEOUT;
        let mut sink = None;
        let called = self.guarded(|| -> rquickjs::Result<Value<'js>> {
            let volt: Object = ctx.globals().get("__VOLT__")?;
            let function: Function = volt.get(job.method)?;
            match &job.reply {
                Reply::Call(_) => function.call((job.payload.as_str(),)),
                Reply::Stream(channel) => {
                    let object = sink_object(ctx, channel)?;
                    sink = Some(object.clone());
                    function.call((job.payload.as_str(), object))
                }
            }
        });
        match called {
            Ok(value) => match value.as_promise() {
                Some(promise) => {
                    Some(Active { promise: promise.clone(), reply: job.reply, sink, deadline, aborted: false })
                }
                None => {
                    job.reply.finish(value.get::<String>().map_err(|e| describe(ctx, e)));
                    None
                }
            },
            Err(error) => {
                job.reply.finish(Err(describe(ctx, error)));
                None
            }
        }
    }

    /// Replies to settled jobs, aborts or fails timed-out ones and returns the ones still pending. An
    /// aborted render still completes its document, with the boundaries left pending rendered in the browser.
    fn settle<'js>(&self, ctx: &Ctx<'js>, active: Vec<Active<'js>>) -> Vec<Active<'js>> {
        let mut pending = Vec::with_capacity(active.len());
        for mut job in active {
            match job.promise.state() {
                PromiseState::Pending if Instant::now() > job.deadline => {
                    let reason = format!("render timed out after {}s", RENDER_TIMEOUT.as_secs());
                    if self.guarded(|| job.abort(&reason)) {
                        pending.push(job);
                    } else {
                        job.reply.finish(Err(reason));
                    }
                }
                PromiseState::Pending => pending.push(job),
                _ => {
                    let result = job.promise.result::<String>().unwrap_or(Err(rquickjs::Error::Exception));
                    job.reply.finish(result.map_err(|e| describe(ctx, e)));
                }
            }
        }
        pending
    }

    /// Runs JavaScript under the watchdog, which interrupts code that runs too long without yielding.
    fn guarded<T>(&self, run: impl FnOnce() -> T) -> T {
        self.set_deadline(Some(Instant::now() + RENDER_TIMEOUT));
        let result = run();
        self.set_deadline(None);
        result
    }

    fn set_deadline(&self, value: Option<Instant>) {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = value;
        }
    }
}

/// The `{ shell, write }` object a streaming render reports its output to.
fn sink_object<'js>(ctx: &Ctx<'js>, sink: &Arc<Sink>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    let shell = sink.clone();
    object.set("shell", Function::new(ctx.clone(), move |meta: String| shell.send(StreamEvent::Shell(meta)))?)?;
    let write = sink.clone();
    object.set(
        "write",
        Function::new(ctx.clone(), move |chunk: TypedArray<'js, u8>| {
            // SAFETY: the bytes are copied before any JavaScript runs again.
            let bytes = unsafe { chunk.as_bytes() }.map(Bytes::copy_from_slice).unwrap_or_default();
            write.send(StreamEvent::Chunk(bytes))
        })?,
    )?;
    Ok(object)
}

fn install_host<'js>(ctx: &Ctx<'js>, env: &str) -> rquickjs::Result<()> {
    let host = Object::new(ctx.clone())?;
    host.set("env", env)?;
    host.set(
        "log",
        Function::new(ctx.clone(), |level: String, message: String| {
            crate::log::js(&level, &message);
        })?,
    )?;
    // Native UTF-8 conversions for TextEncoder and TextDecoder, which React calls for every chunk it writes.
    host.set(
        "encode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: rquickjs::String<'js>| -> rquickjs::Result<Value<'js>> {
            match text.to_string() {
                Ok(text) => TypedArray::<u8>::new(ctx, text.into_bytes()).map(|array| array.into_value()),
                // Lone surrogates have no UTF-8 form: the JavaScript encoder replaces them.
                Err(_) => Ok(Value::new_null(ctx)),
            }
        })?,
    )?;
    host.set(
        "decode",
        Function::new(ctx.clone(), |bytes: TypedArray<'js, u8>| {
            // SAFETY: the bytes are read before any JavaScript runs again.
            unsafe { bytes.as_bytes() }.map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
        })?,
    )?;
    let epoch = Instant::now();
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default();
    host.set("now", Function::new(ctx.clone(), move || base + epoch.elapsed().as_secs_f64() * 1000.0)?)?;
    ctx.globals().set("__volt_host", host)
}

fn eval_file<'js>(ctx: &Ctx<'js>, code: &str, path: &Path) -> rquickjs::Result<()> {
    let mut options = rquickjs::context::EvalOptions::default();
    options.global = true;
    options.strict = false;
    options.filename = Some(path.display().to_string());
    ctx.eval_with_options::<(), _>(code, options)
}

fn describe<'js>(ctx: &Ctx<'js>, error: rquickjs::Error) -> String {
    match CaughtError::from_error(ctx, error) {
        CaughtError::Exception(exception) => {
            let message = exception.message().unwrap_or_default();
            match exception.stack() {
                Some(stack) if !stack.is_empty() => format!("{message}\n{stack}"),
                _ => message,
            }
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &str = r#"
globalThis.__VOLT__ = {
  wait: (ms) => new Promise((resolve) => setTimeout(() => resolve(ms), Number(ms))),
  text: (s) => new TextDecoder().decode(new TextEncoder().encode(s)),
  async read() {
    const stream = new ReadableStream({
      pull(controller) {
        controller.enqueue(new TextEncoder().encode("ab"));
        controller.close();
      },
    });
    let out = "";
    for await (const chunk of stream) out += new TextDecoder().decode(chunk);
    return out;
  },
  stream(payload, sink) {
    sink.shell(JSON.stringify({ status: 200 }));
    sink.write(new TextEncoder().encode(payload));
    return new Promise((resolve) => setTimeout(() => resolve(String(sink.write(new TextEncoder().encode("é")))), 30));
  },
  hang(payload, sink) {
    return new Promise((resolve) => {
      sink.abort = (reason) => resolve(`aborted: ${reason}`);
    });
  },
  stuck: () => new Promise(() => {}),
  chunked() {
    const decoder = new TextDecoder();
    const bytes = new TextEncoder().encode("aé😀");
    let out = "";
    for (const byte of bytes) out += decoder.decode(new Uint8Array([byte]), { stream: true });
    return out + decoder.decode() + new TextDecoder().decode(new TextEncoder().encode("x\ud800y"));
  },
};
"#;

    fn engine(name: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!("volt-ssr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = dir.join("server.js");
        std::fs::write(&bundle, BUNDLE).unwrap();
        Engine::start(EngineOptions { bundle, workers: 1, memory_limit: 64 * 1024 * 1024, dev: false }).unwrap()
    }

    #[tokio::test]
    async fn interleaves_renders_waiting_on_timers() {
        let engine = engine("timers");
        let started = Instant::now();
        let calls: Vec<_> = (0..5)
            .map(|_| {
                let engine = engine.clone();
                tokio::spawn(async move { engine.call("wait", "200".into()).await })
            })
            .collect();
        for call in calls {
            assert_eq!(call.await.unwrap().unwrap(), "200");
        }
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(200), "timers fired early: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(800), "the worker ran the renders one by one: {elapsed:?}");
    }

    #[tokio::test]
    async fn polyfills_encode_and_stream() {
        let engine = engine("polyfills");
        let text = "a é 😀 \u{2028}";
        assert_eq!(engine.call("text", text.into()).await.unwrap(), text);
        assert_eq!(engine.call("read", String::new()).await.unwrap(), "ab");
        assert_eq!(engine.call("chunked", String::new()).await.unwrap(), "aé😀x\u{fffd}y");
    }

    #[tokio::test]
    async fn aborts_renders_that_cannot_settle() {
        let engine = engine("abort");
        let mut events = engine.stream("hang", String::new()).unwrap();
        assert!(matches!(events.recv().await, Some(StreamEvent::Done(Ok(out))) if out.starts_with("aborted: ")));
        let error = engine.call("stuck", String::new()).await.unwrap_err();
        assert!(error.message().contains("never settled"), "{}", error.message());
    }

    #[tokio::test]
    async fn streams_events_in_order() {
        let engine = engine("stream");
        let mut events = engine.stream("stream", "<p>".into()).unwrap();
        assert!(matches!(events.recv().await, Some(StreamEvent::Shell(meta)) if meta.contains("200")));
        assert!(matches!(events.recv().await, Some(StreamEvent::Chunk(bytes)) if bytes == "<p>".as_bytes()));
        assert!(matches!(events.recv().await, Some(StreamEvent::Chunk(bytes)) if bytes == "é".as_bytes()));
        assert!(matches!(events.recv().await, Some(StreamEvent::Done(Ok(written))) if written == "true"));
    }
}
