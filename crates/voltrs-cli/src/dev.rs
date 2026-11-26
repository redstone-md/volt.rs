//! `volt dev`: incremental rebuilds, server restarts and live reload.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;
use voltrs::log;

use crate::pipeline::Pipeline;
use crate::project::Project;

const IGNORED: [&str; 5] = ["/.volt/", "/node_modules/", "/target/", "/.git/", "/dist/"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    Frontend,
    Rust,
    Config,
}

fn classify(path: &Path) -> Option<Change> {
    let text = crate::util::to_slash(path);
    if IGNORED.iter().any(|dir| text.contains(dir)) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy();
    if name == "volt.toml" || name.starts_with(".env") {
        return Some(Change::Config);
    }
    if name == "Cargo.toml" || name == "Cargo.lock" || path.extension().is_some_and(|e| e == "rs") {
        return Some(Change::Rust);
    }
    if name.ends_with('~') || name.starts_with(".#") || name.ends_with(".swp") {
        return None;
    }
    Some(Change::Frontend)
}

enum Process {
    Child(Child),
    InProcess(tokio::task::JoinHandle<()>),
}

/// The running application server, stopped on drop.
#[derive(Default)]
struct Server(Option<Process>);

impl Server {
    fn start(project: &Project, binary: Option<&Path>) -> Self {
        let process = match binary {
            Some(binary) => {
                match Command::new(binary).env("VOLT_ROOT", &project.root).current_dir(&project.root).spawn() {
                    Ok(child) => Some(Process::Child(child)),
                    Err(error) => {
                        log::error(&format!("cannot start {}: {error}", binary.display()));
                        None
                    }
                }
            }
            None => {
                let root = project.root.clone();
                Some(Process::InProcess(tokio::spawn(async move {
                    if let Err(error) = voltrs::App::new().root(root).serve().await {
                        log::error(error.message());
                    }
                })))
            }
        };
        Self(process)
    }

    fn stop(&mut self) {
        match self.0.take() {
            Some(Process::Child(mut child)) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Some(Process::InProcess(handle)) => handle.abort(),
            None => {}
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Session<'a> {
    project: &'a Project,
    pipeline: Pipeline<'a>,
    binary: Option<PathBuf>,
    server: Server,
}

impl<'a> Session<'a> {
    fn compile(&mut self) -> bool {
        if !self.project.rust {
            return true;
        }
        log::info("compiling server…");
        match crate::cargo::build(&self.project.root, false) {
            Ok(binary) => {
                self.binary = Some(binary);
                true
            }
            Err(error) => {
                log::error(&format!("{error:#}"));
                false
            }
        }
    }

    async fn frontend(&self) -> bool {
        let result = match self.pipeline.sdk(self.binary.as_deref()) {
            Ok(info) => self.pipeline.frontend(&info).await.map(|_| ()),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                log::error(&format!("{error:#}"));
                self.pipeline.record_error(&error);
                false
            }
        }
    }

    fn restart(&mut self) {
        self.server.stop();
        self.server = Server::start(self.project, self.binary.as_deref());
    }

    async fn apply(&mut self, change: Change) {
        match change {
            Change::Config => log::warn("volt.toml or .env changed; restart `volt dev` to apply it"),
            Change::Frontend => {
                self.frontend().await;
            }
            Change::Rust => {
                if self.compile() && self.frontend().await {
                    self.restart();
                }
            }
        }
    }
}

pub async fn run(project: Project) -> Result<()> {
    let mut session =
        Session { pipeline: Pipeline::new(&project, true), project: &project, binary: None, server: Server::default() };
    let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && !matches!(event.kind, notify::EventKind::Access(_))
        {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })?;
    watcher.watch(&project.root, RecursiveMode::Recursive)?;

    let mut ready = session.compile() && session.frontend().await;
    if ready {
        session.restart();
    } else {
        log::warn("fix the error above; volt is watching for changes");
    }

    loop {
        let first = tokio::select! {
            path = rx.recv() => path,
            _ = shutdown() => None,
        };
        let Some(first) = first else { break };
        tokio::time::sleep(Duration::from_millis(60)).await;
        let mut changes: Vec<Change> = classify(&first).into_iter().collect();
        while let Ok(path) = rx.try_recv() {
            changes.extend(classify(&path));
        }
        let Some(change) = changes.into_iter().max() else { continue };
        if ready {
            session.apply(change).await;
        } else if session.compile() && session.frontend().await {
            ready = true;
            session.restart();
        }
    }
    session.server.stop();
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        assert_eq!(classify(Path::new("/p/app/page.tsx")), Some(Change::Frontend));
        assert_eq!(classify(Path::new("/p/server/main.rs")), Some(Change::Rust));
        assert_eq!(classify(Path::new("/p/volt.toml")), Some(Change::Config));
        assert_eq!(classify(Path::new("/p/.volt/manifest.json")), None);
        assert_eq!(classify(Path::new("/p/node_modules/react/index.js")), None);
    }
}
