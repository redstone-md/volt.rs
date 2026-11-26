//! The project's own Rust server crate.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
struct Message {
    reason: String,
    executable: Option<PathBuf>,
    target: Option<Target>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
}

/// Builds the server crate and returns the path of its binary.
pub fn build(root: &Path, release: bool) -> Result<PathBuf> {
    let mut command = Command::new("cargo");
    command.args(["build", "--message-format=json-render-diagnostics"]).current_dir(root);
    if release {
        command.arg("--release");
    }
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .context("cannot run cargo; install Rust from https://rustup.rs")?;
    if !output.status.success() {
        bail!("cargo build failed");
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Message>(line).ok())
        .filter(|m| {
            m.reason == "compiler-artifact" && m.target.as_ref().is_some_and(|t| t.kind.iter().any(|k| k == "bin"))
        })
        .filter_map(|m| m.executable)
        .last()
        .context("the server crate has no binary target")
}

/// Asks the server binary to write the TypeScript SDK.
pub fn emit_sdk(binary: &Path, root: &Path, dir: &Path) -> Result<()> {
    let status = Command::new(binary)
        .arg("--emit-sdk")
        .arg(dir)
        .env("VOLT_ROOT", root)
        .current_dir(root)
        .status()
        .with_context(|| format!("cannot run {}", binary.display()))?;
    if !status.success() {
        bail!("{} --emit-sdk failed", binary.display());
    }
    Ok(())
}
