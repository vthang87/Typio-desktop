use unicode_normalization::UnicodeNormalization;

/// Lowercases, strips accents (`đ` becomes `d`) and collapses everything that is not
/// `a-z0-9` into single spaces.
pub fn normalize_vietnamese(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;

    for character in value.trim().to_lowercase().nfd() {
        let character = match character {
            '\u{0300}'..='\u{036f}' => continue,
            'đ' => 'd',
            other => other,
        };
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(character);
        } else {
            pending_space = true;
        }
    }

    out
}

pub fn get_initials(value: &str) -> String {
    initials_of(&normalize_vietnamese(value))
}

/// Initials of an already normalized value.
pub(crate) fn initials_of(normalized: &str) -> String {
    normalized
        .split(' ')
        .filter_map(|part| part.chars().next())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    Prefix,
    Initials,
    Contains,
}

pub fn get_match_kind(value: &str, query: &str) -> Option<MatchKind> {
    let normalized_value = normalize_vietnamese(value);
    let normalized_query = normalize_vietnamese(query);

    if normalized_query.is_empty() {
        return None;
    }
    if normalized_value == normalized_query {
        return Some(MatchKind::Exact);
    }
    if normalized_value.starts_with(&normalized_query) {
        return Some(MatchKind::Prefix);
    }
    if initials_of(&normalized_value).starts_with(&normalized_query.replace(' ', "")) {
        return Some(MatchKind::Initials);
    }
    if normalized_value.contains(&normalized_query) {
        return Some(MatchKind::Contains);
    }
    None
}
