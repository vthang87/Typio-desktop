//! Fields Typio never suggests in. The patterns match the extension's `sensitive.ts`;
//! platforms add their own native checks (password field roles) on top.

use std::sync::LazyLock;

use regex::Regex;

static SENSITIVE_TEXT: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"pass(word|code)?",
        r"mật[ -]?khẩu",
        r"otp",
        r"one[ -]?time[ -]?(code|password)",
        r"pin",
        r"cvv|cvc",
        r"credit[ -]?card|card[ -]?number",
        r"số[ -]?thẻ",
        r"secret|api[ -_]?key|access[ -_]?token",
        r"verification[ -]?code|mã[ -]?xác[ -]?minh",
    ]
    .iter()
    .map(|pattern| Regex::new(&format!("(?i){pattern}")).expect("valid pattern"))
    .collect()
});

/// True when a field's name, identifier or label suggests a password, payment detail,
/// verification code or secret.
pub fn is_sensitive_label(text: &str) -> bool {
    SENSITIVE_TEXT.iter().any(|pattern| pattern.is_match(text))
}
