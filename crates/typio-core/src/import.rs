use std::collections::HashSet;

use serde_json::Value;

use crate::types::{DatasetKind, Shortcut, SuggestionDataset};

/// Trims values, drops empty ones and keeps the first of each duplicate.
pub fn unique_suggestion_values<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.as_ref().trim().to_string())
        .filter(|value| !value.is_empty() && seen.insert(value.clone()))
        .collect()
}

pub fn parse_text_suggestions(text: &str) -> Vec<String> {
    unique_suggestion_values(lines(text))
}

pub fn parse_csv_suggestions(text: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                cell.push('"');
                characters.next();
            }
            '"' => quoted = !quoted,
            ',' | '\n' | '\r' if !quoted => {
                cells.push(std::mem::take(&mut cell));
                if character == '\r' && characters.peek() == Some(&'\n') {
                    characters.next();
                }
            }
            other => cell.push(other),
        }
    }

    cells.push(cell);
    unique_suggestion_values(cells)
}

/// Collects every string and number in a JSON document, in document order.
pub fn parse_json_suggestions(text: &str) -> Result<Vec<String>, serde_json::Error> {
    fn collect(value: &Value, values: &mut Vec<String>) {
        match value {
            Value::String(text) => values.push(text.clone()),
            Value::Number(number) => values.push(format_number(number)),
            Value::Array(items) => items.iter().for_each(|item| collect(item, values)),
            Value::Object(map) => map.values().for_each(|item| collect(item, values)),
            Value::Bool(_) | Value::Null => {}
        }
    }

    let mut values = Vec::new();
    collect(&serde_json::from_str(text)?, &mut values);
    Ok(unique_suggestion_values(values))
}

pub fn parse_suggestion_file(filename: &str, text: &str) -> Result<Vec<String>, serde_json::Error> {
    let text = strip_bom(text);
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    match extension.as_str() {
        "json" => parse_json_suggestions(text),
        "csv" => Ok(parse_csv_suggestions(text)),
        _ => Ok(parse_text_suggestions(text)),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutImport {
    pub shortcuts: Vec<Shortcut>,
    /// Non-empty lines read from the source.
    pub total: usize,
    /// Triggers that appeared again; the first occurrence is kept.
    pub duplicates: Vec<String>,
    /// 1-based line numbers that are not `trigger:expansion`.
    pub invalid_lines: Vec<usize>,
    /// Single-character triggers, accepted but easy to hit by accident.
    pub short_triggers: Vec<String>,
}

/// Parses one `trigger:expansion` per line; the expansion may itself contain ":".
pub fn parse_shortcuts(text: &str) -> ShortcutImport {
    let mut result = ShortcutImport::default();
    let mut seen = HashSet::new();

    for (index, raw_line) in lines(strip_bom(text)).enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        result.total += 1;

        let Some((trigger, expansion)) = split_shortcut_line(line) else {
            result.invalid_lines.push(index + 1);
            continue;
        };
        if !seen.insert(trigger.to_lowercase()) {
            result.duplicates.push(trigger.to_string());
            continue;
        }
        if trigger.chars().count() < 2 {
            result.short_triggers.push(trigger.to_string());
        }
        result.shortcuts.push(Shortcut {
            trigger: trigger.to_string(),
            expansion: expansion.to_string(),
        });
    }

    result
}

/// True when every non-empty line looks like `trigger:expansion`.
pub fn looks_like_shortcuts(text: &str) -> bool {
    let mut lines = lines(strip_bom(text))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    lines.peek().is_some() && lines.all(|line| split_shortcut_line(line).is_some())
}

pub fn format_shortcuts(shortcuts: &[Shortcut]) -> String {
    shortcuts
        .iter()
        .map(|shortcut| format!("{}:{}", shortcut.trigger, shortcut.expansion))
        .collect::<Vec<_>>()
        .join("\n")
}

/// File content for one dataset, in the same format the importer reads back.
pub fn format_dataset_export(dataset: &SuggestionDataset) -> String {
    let body = match dataset.kind() {
        DatasetKind::Shortcuts => {
            format_shortcuts(dataset.shortcuts.as_deref().unwrap_or_default())
        }
        DatasetKind::Values => dataset.values.join("\n"),
    };
    format!("{body}\n")
}

/// Splits a trimmed line on its first ":". The trigger has no spaces or colons and may be
/// followed by spaces; the expansion is everything after, trimmed.
fn split_shortcut_line(line: &str) -> Option<(&str, &str)> {
    let (trigger, expansion) = line.split_once(':')?;
    let trigger = trigger.trim_end();
    let expansion = expansion.trim();
    if trigger.is_empty() || trigger.chars().any(char::is_whitespace) || expansion.is_empty() {
        return None;
    }
    Some((trigger, expansion))
}

/// Splits on `\n` and `\r\n`, like the extension's `/\r?\n/`.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Formats numbers like JavaScript's `String(number)` for everyday values: `3.0` becomes "3".
fn format_number(number: &serde_json::Number) -> String {
    match number.as_f64() {
        Some(float) if number.is_f64() => float.to_string(),
        _ => number.to_string(),
    }
}
