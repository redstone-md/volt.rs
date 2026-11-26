use std::path::Path;

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

pub fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
}
