//! What the suggestion popup shows for a field and what choosing an item writes back.
//! Mirrors the extension's `suggest-engine.ts`. Offsets are UTF-16 code units, the unit
//! macOS Accessibility and Windows UI Automation report carets in.

use crate::rank::{
    build_index, build_shortcut_index, merge_ranked, rank_suggestions, IndexedValue,
    RankedSuggestion, ReplaceScope,
};
use crate::types::{DatasetKind, SuggestionDataset};

pub const MAX_RESULTS: usize = 8;

/// The word right before the caret, which a shortcut replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordRange {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// A replacement of `length` UTF-16 units at `start` with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub start: usize,
    pub length: usize,
    pub text: String,
}

/// Indexed values and shortcuts for one field.
#[derive(Debug, Clone, Default)]
pub struct Suggester {
    values: Vec<IndexedValue>,
    shortcuts: Vec<IndexedValue>,
}

impl Suggester {
    pub fn new<'a>(datasets: impl IntoIterator<Item = &'a SuggestionDataset>) -> Self {
        let mut values = Vec::new();
        let mut shortcuts = Vec::new();
        for dataset in datasets {
            match dataset.kind() {
                DatasetKind::Values => values.extend(dataset.values.iter().cloned()),
                DatasetKind::Shortcuts => {
                    shortcuts.extend(dataset.shortcuts.iter().flatten().cloned())
                }
            }
        }
        Self {
            values: build_index(&values),
            shortcuts: build_shortcut_index(&shortcuts),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.shortcuts.is_empty()
    }

    /// Suggestions for a field holding `text`, with `word` the word before the caret.
    pub fn suggest(&self, text: &str, word: Option<&WordRange>) -> Vec<RankedSuggestion> {
        let mut lists = vec![
            rank_suggestions(&self.values, text, MAX_RESULTS, ReplaceScope::Field),
            rank_suggestions(&self.shortcuts, text, MAX_RESULTS, ReplaceScope::Field),
        ];
        // Inside longer text a shortcut only replaces the word being typed.
        if let Some(word) = word.filter(|word| word.text != text.trim()) {
            lists.push(rank_suggestions(
                &self.shortcuts,
                &word.text,
                MAX_RESULTS,
                ReplaceScope::Word,
            ));
        }
        merge_ranked(lists, MAX_RESULTS)
    }
}

/// Characters that end a word for shortcut matching; triggers never contain them.
fn is_word_break(character: char) -> bool {
    character.is_whitespace() || ".,;:!?()[]{}\"'“”".contains(character)
}

/// The word ending at `caret` (a UTF-16 offset), or `None` when the caret sits inside a word.
pub fn word_at(text: &str, caret: usize) -> Option<WordRange> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    if caret > utf16.len() {
        return None;
    }
    let before = String::from_utf16_lossy(&utf16[..caret]);
    let after = String::from_utf16_lossy(&utf16[caret..]);

    // A caret in the middle of a word never triggers a shortcut.
    if after
        .chars()
        .next()
        .is_some_and(|next| !is_word_break(next))
    {
        return None;
    }
    let word_start = before
        .char_indices()
        .rev()
        .find(|(_, character)| is_word_break(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let word = &before[word_start..];
    if word.is_empty() {
        return None;
    }
    let length = word.encode_utf16().count();
    Some(WordRange {
        text: word.to_string(),
        start: caret - length,
        end: caret,
    })
}

/// The word typed after a `:` that ends at `caret`, for example `xtt` in `Giao tới :xtt`.
/// The range covers the colon too, so choosing a suggestion replaces `:xtt`. The colon must
/// start the field or follow whitespace, so `10:30` and `http://` are left alone.
pub fn prefixed_word_at(text: &str, caret: usize) -> Option<WordRange> {
    let word = word_at(text, caret)?;
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let colon = word.start.checked_sub(1)?;
    if utf16[colon] != u16::from(b':') {
        return None;
    }
    let before = String::from_utf16_lossy(&utf16[..colon]);
    if before
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace())
    {
        return None;
    }
    Some(WordRange {
        start: colon,
        ..word
    })
}

/// What choosing `choice` writes into a field holding `text`.
pub fn replacement_for(
    text: &str,
    choice: &RankedSuggestion,
    word: Option<&WordRange>,
) -> Replacement {
    match (choice.scope, word) {
        (ReplaceScope::Word, Some(word)) => Replacement {
            start: word.start,
            length: word.end - word.start,
            text: choice.value.clone(),
        },
        _ => Replacement {
            start: 0,
            length: text.encode_utf16().count(),
            text: choice.value.clone(),
        },
    }
}
