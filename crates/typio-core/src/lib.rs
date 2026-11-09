//! Platform-independent Typio logic: Vietnamese normalization, matching and ranking,
//! dataset import and backup reading. Behaviour matches the Typio browser extension.

pub mod backup;
pub mod engine;
pub mod import;
pub mod normalize;
pub mod rank;
pub mod sensitive;
pub mod types;

pub use normalize::{get_initials, get_match_kind, normalize_vietnamese, MatchKind};
pub use rank::{
    build_index, build_shortcut_index, merge_ranked, rank_suggestions, IndexedValue,
    RankedSuggestion, ReplaceScope, SuggestionKind,
};
pub use types::{DatasetKind, Shortcut, SuggestionDataset};
