//! React rendering on a pool of QuickJS workers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::{CaughtError, Context, Ctx, Function, Object, Runtime, Value, promise::PromiseState};
use tokio::sync::oneshot;

use crate::error::{Error, Result};

const POLYFILLS: &str = include_str!("polyfills.js");
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TICKS: usize = 100_000;
const GC_THRESHOLD: usize = 8 * 1024 * 1024;

struct Job {
    method: &'static str,
    payload: String,
    reply: oneshot::Sender<Result<String, String>>,
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
        self.jobs.send(Job { method, payload, reply }).map_err(|_| Error::internal("SSR pool is not running"))?;
        rx.await.map_err(|_| Error::internal("SSR worker dropped the request"))?.map_err(Error::internal)
    }

    /// Makes every worker evaluate `bundle` before its next job.
    pub fn reload(&self, bundle: PathBuf) {
        if let Ok(mut current) = self.shared.bundle.write() {
            *current = bundle;
        }
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
    }
}

struct Worker {
    /// Owns the QuickJS heap the context lives in.
    _runtime: Runtime,
    context: Context,
    generation: u64,
    deadline: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl Worker {
    fn run(
        options: EngineOptions,
        jobs: flume::Receiver<Job>,
        shared: Arc<Shared>,
        ready: std::sync::mpsc::Sender<Result<(), String>>,
    ) {
        let mut worker = match Self::load(&options, &shared.bundle(), shared.generation.load(Ordering::SeqCst)) {
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
            let current = shared.generation.load(Ordering::SeqCst);
            if current != worker.generation {
                match Self::load(&options, &shared.bundle(), current) {
                    Ok(fresh) => worker = fresh,
                    Err(message) => {
                        let _ = job.reply.send(Err(message));
                        continue;
                    }
                }
            }
            let result = worker.call(job.method, &job.payload);
            let _ = job.reply.send(result);
        }
    }

    fn load(options: &EngineOptions, bundle: &Path, generation: u64) -> Result<Self, String> {
        let code = std::fs::read_to_string(bundle)
            .map_err(|e| format!("cannot read server bundle {}: {e}", bundle.display()))?;
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(options.memory_limit);
        runtime.set_gc_threshold(GC_THRESHOLD);
        runtime.set_max_stack_size(8 * 1024 * 1024);
        let deadline: Arc<std::sync::Mutex<Option<Instant>>> = Arc::default();
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

    fn call(&mut self, method: &str, payload: &str) -> Result<String, String> {
        self.set_deadline(Some(Instant::now() + RENDER_TIMEOUT));
        let result = self.context.with(|ctx| -> Result<String, String> {
            let run = || -> rquickjs::Result<String> {
                let volt: Object = ctx.globals().get("__VOLT__")?;
                let function: Function = volt.get(method)?;
                let value: Value = function.call((payload,))?;
                settle(&ctx, value)
            };
            run().map_err(|e| describe(&ctx, e))
        });
        self.set_deadline(None);
        result
    }

    fn set_deadline(&self, value: Option<Instant>) {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = value;
        }
    }
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

/// Drives the job queue and timers until a returned promise settles.
fn settle<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<String> {
    let Some(promise) = value.as_promise().cloned() else {
        return value.get();
    };
    let tick: Function = ctx.globals().get("__volt_tick")?;
    for _ in 0..MAX_TICKS {
        match promise.state() {
            PromiseState::Pending => {
                if !ctx.execute_pending_job() && tick.call::<_, i32>(())? == 0 {
                    return Err(rquickjs::Error::new_from_js_message("promise", "string", "render never settled"));
                }
            }
            _ => {
                return promise.result::<String>().unwrap_or(Err(rquickjs::Error::Exception));
            }
        }
    }
    Err(rquickjs::Error::new_from_js_message("promise", "string", "render exceeded the tick budget"))
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
