//! The `volt` command.

mod bundler;
mod cargo;
mod compress;
mod css;
mod csstext;
mod dev;
mod env;
mod export;
mod fonts;
mod media;
mod new;
mod pipeline;
mod postcss;
mod project;
mod routes;
mod styles;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use voltrs::log;

use crate::export::Export;
use crate::pipeline::Pipeline;
use crate::project::Project;

#[derive(Parser)]
#[command(name = "volt", version, about = "React on a Rust server. SSR, server actions and file-system routing.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project
    New {
        path: PathBuf,
        /// `default` (React + Rust server) or `web` (React only)
        #[arg(long, short, default_value = "default", value_parser = new::TEMPLATE_NAMES)]
        template: String,
        /// Skip installing npm dependencies
        #[arg(long)]
        no_install: bool,
    },
    /// Start the development server with live reload
    Dev {
        #[arg(long, short)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
    },
    /// Build for production
    Build,
    /// Build a static site into `[build] export_dir` (default `out/`)
    Export,
    /// Run the production server
    Start {
        #[arg(long, short)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
    },
    /// Regenerate the typed `@volt/api` client
    Sdk,
    /// List the app routes
    Routes,
}

fn set_env(key: &str, value: Option<String>) {
    if let Some(value) = value {
        // SAFETY: set before the runtime spawns any threads that read the environment.
        unsafe { std::env::set_var(key, value) };
    }
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Command::Dev { port, host } => {
            set_env("VOLT_DEV", Some("1".into()));
            set_env("PORT", port.map(|p| p.to_string()));
            set_env("HOST", host.clone());
        }
        Command::Start { port, host } => {
            set_env("PORT", port.map(|p| p.to_string()));
            set_env("HOST", host.clone());
        }
        Command::Export => set_env("VOLT_EXPORT", Some("1".into())),
        _ => {}
    }
    if !matches!(cli.command, Command::Export) {
        // SAFETY: single-threaded at this point.
        unsafe { std::env::remove_var("VOLT_EXPORT") };
    }
    if !matches!(cli.command, Command::Dev { .. }) {
        // SAFETY: single-threaded at this point.
        unsafe { std::env::remove_var("VOLT_DEV") };
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(32 * 1024 * 1024)
        .build()
        .expect("tokio runtime");
    let result = runtime.block_on(async { tokio::spawn(run(cli.command)).await });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::error(&format!("{error:#}"));
            std::process::exit(1);
        }
        Err(panic) => {
            log::error(&format!("volt crashed: {panic}"));
            std::process::exit(101);
        }
    }
}

async fn run(command: Command) -> Result<()> {
    let cwd = std::env::current_dir()?;
    match command {
        Command::New { path, template, no_install } => new::run(&path, &template, !no_install),
        Command::Dev { .. } => {
            banner("dev");
            dev::run(Project::discover(&cwd)?).await
        }
        Command::Build => {
            banner("build");
            build(&Project::discover(&cwd)?).await.map(|_| ())
        }
        Command::Export => {
            banner("export");
            export(&Project::discover(&cwd)?).await
        }
        Command::Start { .. } => start(&Project::discover(&cwd)?).await,
        Command::Sdk => {
            let project = Project::discover(&cwd)?;
            let binary = if project.rust { Some(cargo::build(&project.root, false)?) } else { None };
            Pipeline::new(&project, false).sdk(binary.as_deref())?;
            log::ready(&format!("wrote {}", project.relative(&project.sdk_file())));
            Ok(())
        }
        Command::Routes => routes(&Project::discover(&cwd)?),
    }
}

fn banner(mode: &str) {
    eprintln!("\n  {} {}  {}\n", log::paint("1;33", "⚡ volt"), env!("CARGO_PKG_VERSION"), log::paint("2", mode));
}

async fn build(project: &Project) -> Result<(voltrs::sdk::ServerInfo, voltrs::manifest::Manifest)> {
    let binary = if project.rust {
        log::info("compiling server (release)…");
        Some(cargo::build(&project.root, true)?)
    } else {
        None
    };
    let pipeline = Pipeline::new(project, false);
    let info = pipeline.sdk(binary.as_deref())?;
    let manifest = pipeline.frontend(&info).await?;
    if let Some(binary) = binary {
        log::ready(&format!("server binary: {}", project.relative(&binary)));
    }
    Ok((info, manifest))
}

async fn export(project: &Project) -> Result<()> {
    Export::check_output(project, &project.config.export_dir())?;
    let (info, manifest) = build(project).await?;
    let export = Export::new(project, &manifest, &info);
    let blockers = export.blockers();
    if !blockers.is_empty() {
        anyhow::bail!("cannot export a static site, these need the server:\n  - {}", blockers.join("\n  - "));
    }
    for warning in export.ignored() {
        log::warn(&warning);
    }
    let pages = export.write().await?;
    let out = project.config.export_dir();
    log::ready(&format!("exported {pages} pages to {}", project.relative(&out)));
    Ok(())
}

async fn start(project: &Project) -> Result<()> {
    if !project.out_dir.join(voltrs::manifest::MANIFEST_FILE).is_file() {
        anyhow::bail!("no build found; run `volt build` first");
    }
    if project.rust {
        let binary = cargo::build(&project.root, true)?;
        let status =
            std::process::Command::new(&binary).env("VOLT_ROOT", &project.root).current_dir(&project.root).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
    voltrs::App::new().root(&project.root).serve().await.map_err(|e| anyhow::anyhow!("{}", e.message()))
}

fn routes(project: &Project) -> Result<()> {
    let info: voltrs::sdk::ServerInfo =
        std::fs::read_to_string(project.types_dir().join(voltrs::sdk::SERVER_INFO_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
    for route in project.routes()?.iter().filter(|r| !r.is_special()) {
        let kind = if info.loaders.contains(&route.id) {
            "loader"
        } else if route.is_dynamic() {
            "dynamic"
        } else {
            "static"
        };
        let file = match &route.page {
            routes::Module::File(path) => project.relative(path).into_owned(),
            routes::Module::Builtin(name) => name.to_string(),
        };
        let router = if route.is_pages() { "pages" } else { "app" };
        println!("  {:<32} {:<8} {:<6} {}", route.id, kind, router, log::paint("2", &file));
    }
    for api in &info.api {
        println!("  {:<32} {:<8}", api, "api");
    }
    Ok(())
}
