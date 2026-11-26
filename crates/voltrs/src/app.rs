use std::future::Future;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::{Ctx, State};
use crate::error::{Error, Result};
use crate::response::Response;
use crate::sdk::Sdk;
use crate::server::{self, Middleware, Server};
use crate::{config, log};

/// The application: state, middleware and the server entry point.
pub struct App {
    root: PathBuf,
    state: State,
    middleware: Vec<Middleware>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let root = std::env::var_os("VOLT_ROOT")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        Self { root, state: State::default(), middleware: Vec::new() }
    }

    /// Project directory containing `volt.toml` and `.volt/`.
    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self
    }

    /// Registers shared state, available through [`Ctx::state`].
    pub fn state<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state.insert(value);
        self
    }

    /// Runs before routing. Returning `Some` short-circuits the request.
    pub fn middleware<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Ctx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<Response>> + Send + 'static,
    {
        self.middleware.push(std::sync::Arc::new(move |ctx| Box::pin(handler(ctx))));
        self
    }

    /// Blocking entry point for `fn main`. Also answers `--emit-sdk <dir>` for the CLI.
    pub fn run(self) {
        let args: Vec<String> = std::env::args().collect();
        if let Some(index) = args.iter().position(|a| a == "--emit-sdk") {
            let dir =
                args.get(index + 1).map(PathBuf::from).unwrap_or_else(|| self.root.join(config::OUT_DIR).join("types"));
            if let Err(error) = Sdk::generate().write(&dir) {
                log::error(&format!("cannot write SDK: {error}"));
                std::process::exit(1);
            }
            return;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                log::error(&format!("cannot start tokio: {error}"));
                std::process::exit(1);
            }
        };
        if let Err(error) = runtime.block_on(self.serve()) {
            log::error(error.message());
            std::process::exit(1);
        }
    }

    /// Binds the configured address and serves until SIGINT or SIGTERM.
    pub async fn serve(self) -> Result<()> {
        let config = Config::load(&self.root)?;
        let address = format!("{}:{}", config.server.host, config.server.port);
        let port = config.server.port;
        let server = Server::new(config, self.state, self.middleware)?;
        if config::is_dev() {
            server.watch();
        }
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| Error::internal(format!("cannot bind {address}: {e}")))?;
        log::ready(&format!("ready on http://localhost:{port}"));
        server::serve(server, listener).await.map_err(Error::from)
    }

    /// Renders the given static routes to HTML without starting a listener.
    pub async fn prerender(self, routes: &[String]) -> Result<Vec<(String, Result<String>)>> {
        let config = Config::load(&self.root)?;
        let server = Server::new(config, self.state, self.middleware)?;
        Ok(server.prerender(routes).await)
    }
}
