//! Reads backups exported by the Typio extension, and writes datasets backups it can read. Field rules describe web page fields, so
//! the desktop app keeps them as raw JSON and only uses the datasets.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value;

use crate::import::strip_bom;
use crate::types::SuggestionDataset;

pub const BACKUP_SCHEMA_VERSION: u64 = 1;
pub const MAX_BACKUP_BYTES: usize = 10 * 1024 * 1024;

/// `All` restores by replacing everything; `Datasets` restores by merging datasets only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupScope {
    All,
    Datasets,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypioBackup {
    pub scope: BackupScope,
    pub exported_at: String,
    /// Extension field rules, unvalidated. Always empty for a `Datasets` backup.
    pub field_rules: Vec<Value>,
    pub suggestion_datasets: Vec<SuggestionDataset>,
}

/// Messages match the extension's so both apps tell the user the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupError {
    InvalidJson,
    NotTypio,
    UnsupportedVersion(String),
    UnknownScope,
    UnexpectedFieldRules,
    InvalidFieldRule,
    InvalidDataset,
    DuplicateItems,
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson => write!(f, "This file is not valid JSON."),
            Self::NotTypio => write!(f, "This file is not a Typio backup."),
            Self::UnsupportedVersion(version) => write!(
                f,
                "Unsupported backup version {version}. This version of Typio reads version {BACKUP_SCHEMA_VERSION}."
            ),
            Self::UnknownScope => write!(f, "This backup has an unknown type."),
            Self::UnexpectedFieldRules => write!(f, "This datasets backup unexpectedly contains field rules."),
            Self::InvalidFieldRule => write!(f, "The backup contains an invalid field rule."),
            Self::InvalidDataset => write!(f, "The backup contains an invalid dataset."),
            Self::DuplicateItems => write!(f, "The backup contains duplicate items."),
        }
    }
}

impl std::error::Error for BackupError {}

/// A datasets backup in the extension's format, which both apps import.
pub fn format_backup(datasets: &[SuggestionDataset], exported_at: &str) -> String {
    let backup = serde_json::json!({
        "app": "typio",
        "schemaVersion": BACKUP_SCHEMA_VERSION,
        "scope": "datasets",
        "exportedAt": exported_at,
        "suggestionDatasets": datasets,
    });
    let mut text = serde_json::to_string_pretty(&backup).unwrap_or_default();
    text.push('\n');
    text
}

/// Parses and validates a backup file.
pub fn parse_backup(text: &str) -> Result<TypioBackup, BackupError> {
    let data: Value =
        serde_json::from_str(strip_bom(text)).map_err(|_| BackupError::InvalidJson)?;
    let Some(backup) = data
        .as_object()
        .filter(|object| object.get("app") == Some(&Value::from("typio")))
    else {
        return Err(BackupError::NotTypio);
    };

    let version = backup.get("schemaVersion");
    if version.and_then(Value::as_u64) != Some(BACKUP_SCHEMA_VERSION) {
        return Err(BackupError::UnsupportedVersion(
            version.map_or("undefined".to_string(), Value::to_string),
        ));
    }

    // Backups made before scopes existed are full backups.
    let scope = match backup.get("scope") {
        None => BackupScope::All,
        Some(scope) if scope == "all" => BackupScope::All,
        Some(scope) if scope == "datasets" => BackupScope::Datasets,
        Some(_) => return Err(BackupError::UnknownScope),
    };

    let field_rules = match (scope, backup.get("fieldRules")) {
        (BackupScope::Datasets, None) => Vec::new(),
        (_, Some(Value::Array(rules))) => rules.clone(),
        _ => return Err(BackupError::InvalidFieldRule),
    };
    if scope == BackupScope::Datasets && !field_rules.is_empty() {
        return Err(BackupError::UnexpectedFieldRules);
    }
    if !field_rules.iter().all(is_field_rule) {
        return Err(BackupError::InvalidFieldRule);
    }

    let suggestion_datasets: Vec<SuggestionDataset> = backup
        .get("suggestionDatasets")
        .cloned()
        .and_then(|datasets| serde_json::from_value(datasets).ok())
        .ok_or(BackupError::InvalidDataset)?;

    let rule_ids = field_rules
        .iter()
        .filter_map(|rule| rule.get("id").and_then(Value::as_str));
    if has_duplicates(rule_ids)
        || has_duplicates(
            suggestion_datasets
                .iter()
                .map(|dataset| dataset.id.as_str()),
        )
    {
        return Err(BackupError::DuplicateItems);
    }

    Ok(TypioBackup {
        scope,
        exported_at: backup
            .get("exportedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        field_rules,
        suggestion_datasets,
    })
}

/// The extension's structural check for a field rule.
fn is_field_rule(value: &Value) -> bool {
    let is_string = |key: &str| value.get(key).is_some_and(Value::is_string);
    let dataset_ids_ok = match value.get("datasetIds") {
        None => true,
        Some(Value::Array(ids)) => ids.iter().all(Value::is_string),
        Some(_) => false,
    };
    let fingerprint_ok = value.get("fingerprint").is_some_and(|fingerprint| {
        fingerprint.get("selector").is_some_and(Value::is_string)
            && matches!(
                fingerprint.get("tagName").and_then(Value::as_str),
                Some("input" | "textarea" | "contenteditable")
            )
    });

    value.is_object()
        && ["id", "origin", "hostname", "createdAt", "updatedAt"]
            .into_iter()
            .all(is_string)
        && value.get("enabled").is_some_and(Value::is_boolean)
        && dataset_ids_ok
        && fingerprint_ok
}

fn has_duplicates<'a>(ids: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    !ids.into_iter().all(|id| seen.insert(id))
}
