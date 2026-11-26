//! `volt new`: scaffolds a project from the embedded templates.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use include_dir::{Dir, DirEntry, include_dir};
use voltrs::log;

static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/templates");

pub const TEMPLATE_NAMES: [&str; 2] = ["default", "web"];

fn render(text: &str, name: &str) -> String {
    text.replace("{{name}}", name).replace("{{version}}", env!("CARGO_PKG_VERSION"))
}

fn target_name(path: &Path) -> std::path::PathBuf {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) if name.starts_with('_') && !name.starts_with("__") => {
            path.with_file_name(format!(".{}", &name[1..]))
        }
        _ => path.to_path_buf(),
    }
}

fn copy(dir: &Dir<'_>, base: &Path, target: &Path, name: &str) -> Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => copy(child, base, target, name)?,
            DirEntry::File(file) => {
                let relative = file.path().strip_prefix(base).unwrap_or(file.path());
                let destination = target.join(target_name(relative));
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                match file.contents_utf8() {
                    Some(text) => std::fs::write(&destination, render(text, name))?,
                    None => std::fs::write(&destination, file.contents())?,
                }
            }
        }
    }
    Ok(())
}

fn package_manager() -> &'static str {
    let agent = std::env::var("npm_config_user_agent").unwrap_or_default();
    ["pnpm", "yarn", "bun"].into_iter().find(|pm| agent.starts_with(pm)).unwrap_or("npm")
}

pub fn run(path: &Path, template: &str, install: bool) -> Result<()> {
    let dir = TEMPLATES.get_dir(template).with_context(|| format!("unknown template `{template}`"))?;
    if path.exists() && path.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
        bail!("{} already exists and is not empty", path.display());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "volt-app".into());
    let name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    std::fs::create_dir_all(path)?;
    copy(dir, dir.path(), path, &name)?;
    log::ready(&format!("created {} from the `{template}` template", path.display()));

    let pm = package_manager();
    if install {
        log::info(&format!("installing dependencies with {pm}…"));
        let status = Command::new(pm).arg("install").current_dir(path).status();
        if !status.is_ok_and(|s| s.success()) {
            log::warn(&format!("`{pm} install` failed; run it manually"));
        }
    }
    println!("\n  cd {}\n  {pm} run dev\n", path.display());
    Ok(())
}
