use std::collections::{HashMap, HashSet};

use crate::normalize::{initials_of, normalize_vietnamese};
use crate::types::Shortcut;

/// Declaration order is rank order. `Shortcut` is an exact trigger match; `Trigger` is a
/// trigger that starts with the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SuggestionKind {
    Shortcut,
    Exact,
    Prefix,
    Initials,
    Trigger,
    Contains,
}

/// `Field` replaces the whole field value; `Word` replaces only the word before the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplaceScope {
    #[default]
    Field,
    Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedValue {
    pub value: String,
    pub normalized: String,
    pub initials: String,
    /// Lowercased shortcut triggers that expand to this value.
    pub triggers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedSuggestion {
    pub value: String,
    pub kind: SuggestionKind,
    pub scope: ReplaceScope,
    /// The trigger that matched, for `Shortcut` and `Trigger` kinds.
    pub trigger: Option<String>,
}

/// Precomputes search keys once per dataset; deduplicates values that normalize to the same key.
pub fn build_index<S: AsRef<str>>(values: &[S]) -> Vec<IndexedValue> {
    build_entries(values.iter().map(|value| (value.as_ref(), None)))
}

/// Indexes shortcut expansions so they match by trigger as well as by their own text.
pub fn build_shortcut_index(shortcuts: &[Shortcut]) -> Vec<IndexedValue> {
    build_entries(
        shortcuts
            .iter()
            .map(|shortcut| (shortcut.expansion.as_str(), Some(shortcut.trigger.as_str()))),
    )
}

fn build_entries<'a>(items: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Vec<IndexedValue> {
    let mut entries: Vec<IndexedValue> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();

    for (value, trigger) in items {
        let normalized = normalize_vietnamese(value);
        if normalized.is_empty() {
            continue;
        }
        let position = *by_key.entry(normalized.clone()).or_insert_with(|| {
            entries.push(IndexedValue {
                value: value.to_string(),
                initials: initials_of(&normalized),
                normalized,
                triggers: Vec::new(),
            });
            entries.len() - 1
        });
        if let Some(trigger) = trigger {
            entries[position].triggers.push(trigger.to_lowercase());
        }
    }

    entries
}

struct Candidate {
    suggestion: RankedSuggestion,
    literal: bool,
    position: usize,
}

pub fn rank_suggestions(
    index: &[IndexedValue],
    query: &str,
    limit: usize,
    scope: ReplaceScope,
) -> Vec<RankedSuggestion> {
    let normalized_query = normalize_vietnamese(query);
    if normalized_query.is_empty() {
        return Vec::new();
    }

    let compact_query = normalized_query.replace(' ', "");
    let literal_query = query.trim().to_lowercase();
    let mut matches = Vec::new();

    for (position, entry) in index.iter().enumerate() {
        // The field already holds this exact value; suggesting it again only gets in the way.
        if entry.value == query {
            continue;
        }
        let Some((kind, trigger)) =
            get_match(entry, &normalized_query, &compact_query, &literal_query)
        else {
            continue;
        };
        matches.push(Candidate {
            suggestion: RankedSuggestion {
                value: entry.value.clone(),
                kind,
                scope,
                trigger,
            },
            // Queries typed with accents prefer values containing that exact spelling.
            literal: entry.value.to_lowercase().contains(&literal_query),
            position,
        });
    }

    sort_ranked(&mut matches);
    matches
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.suggestion)
        .collect()
}

/// Merges ranked lists, keeping the best match for each value.
pub fn merge_ranked(lists: Vec<Vec<RankedSuggestion>>, limit: usize) -> Vec<RankedSuggestion> {
    let mut merged: Vec<Candidate> = lists
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(position, suggestion)| Candidate {
            suggestion,
            literal: false,
            position,
        })
        .collect();
    sort_ranked(&mut merged);

    let mut seen = HashSet::new();
    merged
        .into_iter()
        .map(|candidate| candidate.suggestion)
        .filter(|suggestion| seen.insert(suggestion.value.clone()))
        .take(limit)
        .collect()
}

fn sort_ranked(items: &mut [Candidate]) {
    items.sort_by(|left, right| {
        left.suggestion
            .kind
            .cmp(&right.suggestion.kind)
            .then(right.literal.cmp(&left.literal))
            .then(left.position.cmp(&right.position))
    });
}

fn get_match(
    entry: &IndexedValue,
    normalized_query: &str,
    compact_query: &str,
    literal_query: &str,
) -> Option<(SuggestionKind, Option<String>)> {
    if entry
        .triggers
        .iter()
        .any(|trigger| trigger == literal_query)
    {
        return Some((SuggestionKind::Shortcut, Some(literal_query.to_string())));
    }
    if entry.normalized == normalized_query {
        return Some((SuggestionKind::Exact, None));
    }
    if entry.normalized.starts_with(normalized_query) {
        return Some((SuggestionKind::Prefix, None));
    }
    if compact_query.chars().count() > 1 && entry.initials.starts_with(compact_query) {
        return Some((SuggestionKind::Initials, None));
    }
    if let Some(trigger) = entry
        .triggers
        .iter()
        .find(|candidate| candidate.starts_with(literal_query))
    {
        return Some((SuggestionKind::Trigger, Some(trigger.clone())));
    }
    if entry.normalized.contains(normalized_query) {
        return Some((SuggestionKind::Contains, None));
    }
    None
}
