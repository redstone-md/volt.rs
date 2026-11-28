use std::path::{Path, PathBuf};

/// FNV-1a 64-bit, rendered as 10 base36 characters. Stable across builds and toolchains.
pub fn hash(bytes: &[u8]) -> String {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x0100_0000_01b3);
    }
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    (0..10)
        .map(|_| {
            let c = alphabet[(value % 36) as usize] as char;
            value /= 36;
            c
        })
        .collect()
}

/// `Path::canonicalize` without the Windows `\\?\` prefix on drive paths. Bundler module ids built from a
/// verbatim root become `//?/C:\...`, which `Path` cannot split into file names or extensions.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let path = path.canonicalize()?;
    if cfg!(windows)
        && let Some(rest) = path.to_str().and_then(|p| p.strip_prefix(r"\\?\"))
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return Ok(PathBuf::from(rest));
    }
    Ok(path)
}

pub fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The slash-separated path from directory `from` to `to`.
pub fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to_parts).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(to_parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    to_slash(Path::new(&parts.join("/")))
}

pub fn reset_dir(dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)
}

pub fn diagnostics(error: &impl std::fmt::Display) -> String {
    error.to_string()
}

/// Warnings that are expected in React apps and only add noise.
pub fn is_noise(warning: &rolldown_error::BuildDiagnostic) -> bool {
    use rolldown_error::EventKind;
    matches!(
        warning.kind(),
        EventKind::ModuleLevelDirective | EventKind::CircularDependency | EventKind::Eval | EventKind::EmptyImportMeta
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn hashes_stably() {
        assert_eq!(super::hash(b"volt"), super::hash(b"volt"));
        assert_ne!(super::hash(b"volt"), super::hash(b"volt!"));
        assert_eq!(super::hash(b"").len(), 10);
    }

    #[test]
    fn relative_paths() {
        use std::path::Path;
        assert_eq!(
            super::relative(Path::new("/p/.volt/cache"), Path::new("/p/app/globals.css")),
            "../../app/globals.css"
        );
        assert_eq!(super::relative(Path::new("/p/app"), Path::new("/p/app/./parts")), "parts");
        assert_eq!(super::relative(Path::new("/p/app"), Path::new("/p/app")), "");
    }
}
