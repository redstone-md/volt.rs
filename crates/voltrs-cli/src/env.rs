//! `.env` files, loaded in Next.js order without overriding the real environment.

use std::path::Path;

const PUBLIC_PREFIXES: [&str; 2] = ["VOLT_PUBLIC_", "NEXT_PUBLIC_"];

pub fn load(root: &Path) {
    let mode = if voltrs::config::is_dev() { "development" } else { "production" };
    let files = [format!(".env.{mode}.local"), ".env.local".into(), format!(".env.{mode}"), ".env".into()];
    for file in files {
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else { continue };
        for (key, value) in parse(&text) {
            if std::env::var_os(&key).is_none() {
                // SAFETY: called on the main thread before any worker threads start.
                unsafe { std::env::set_var(key, value) };
            }
        }
    }
}

fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("export ").unwrap_or(line).split_once('='))
        .map(|(key, value)| {
            let value = value.trim();
            let value = match value.chars().next() {
                Some(q @ ('"' | '\'')) => value.trim_start_matches(q).split(q).next().unwrap_or_default().to_owned(),
                _ => value.split(" #").next().unwrap_or_default().trim().to_owned(),
            };
            (key.trim().to_owned(), value.replace("\\n", "\n"))
        })
        .collect()
}

/// Environment variables inlined into client bundles.
pub fn public() -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> =
        std::env::vars().filter(|(key, _)| PUBLIC_PREFIXES.iter().any(|p| key.starts_with(p))).collect();
    vars.sort();
    vars
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_dotenv() {
        let vars = super::parse("# c\nA=1\nexport B=\"two words\"\nC='x' \nD=raw # note\n");
        assert_eq!(
            vars,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "x".into()),
                ("D".into(), "raw".into())
            ]
        );
    }
}
