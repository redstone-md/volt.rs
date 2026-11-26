//! Minimal terminal logger.

use std::io::{IsTerminal, Write};
use std::sync::OnceLock;
use std::time::Duration;

fn color() -> bool {
    static COLOR: OnceLock<bool> = OnceLock::new();
    *COLOR.get_or_init(|| std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none())
}

pub fn paint(code: &str, text: &str) -> String {
    if color() { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_owned() }
}

fn emit(tag: &str, code: &str, message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{} {message}", paint(code, tag));
}

pub fn info(message: &str) {
    emit(" volt ", "1;30;43", message);
}

pub fn ready(message: &str) {
    emit("  ✓  ", "1;32", message);
}

pub fn warn(message: &str) {
    emit(" warn ", "1;30;43", message);
}

pub fn error(message: &str) {
    emit(" error", "1;37;41", message);
}

pub fn js(level: &str, message: &str) {
    match level {
        "error" => emit("  js  ", "1;31", message),
        "warn" => emit("  js  ", "1;33", message),
        _ => emit("  js  ", "1;36", message),
    }
}

pub fn request(method: &str, path: &str, status: u16, elapsed: Duration) {
    let code = match status {
        200..=299 => "32",
        300..=399 => "36",
        400..=499 => "33",
        _ => "31",
    };
    let micros = elapsed.as_micros();
    let time = if micros < 1000 { format!("{micros}µs") } else { format!("{:.1}ms", micros as f64 / 1000.0) };
    emit("  →  ", "2", &format!("{method} {path} {} {}", paint(code, &status.to_string()), paint("2", &time)));
}
