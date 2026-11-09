use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub trigger: String,
    pub expansion: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatasetKind {
    #[default]
    Values,
    Shortcuts,
}

/// The same shape the extension stores and exports, so backups move between the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionDataset {
    pub id: String,
    pub name: String,
    /// Missing on datasets saved before shortcuts existed; treat as `Values`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DatasetKind>,
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<Vec<Shortcut>>,
    pub created_at: String,
    pub updated_at: String,
}

impl SuggestionDataset {
    pub fn kind(&self) -> DatasetKind {
        self.kind.unwrap_or_default()
    }
}
