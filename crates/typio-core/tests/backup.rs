use serde_json::{json, Value};
use typio_core::backup::{parse_backup, BackupError, BackupScope};
use typio_core::import::{format_dataset_export, parse_shortcuts, parse_text_suggestions};
use typio_core::{DatasetKind, Shortcut, SuggestionDataset};

const STAMP: &str = "2026-11-06T00:00:00.000Z";

fn rule() -> Value {
    json!({
        "id": "r1", "origin": "https://example.com", "hostname": "example.com",
        "fingerprint": { "tagName": "input", "selector": "#name" },
        "datasetIds": ["d1"], "enabled": true, "createdAt": STAMP, "updatedAt": STAMP
    })
}

fn datasets() -> Value {
    json!([
        { "id": "d1", "name": "Tên", "values": ["Nguyễn Văn An"], "createdAt": STAMP, "updatedAt": STAMP },
        { "id": "d2", "name": "Gõ tắt", "kind": "shortcuts", "values": [],
          "shortcuts": [{ "trigger": "pbg", "expansion": "Phường Bắc Giang" }], "createdAt": STAMP, "updatedAt": STAMP }
    ])
}

/// A full backup as the extension writes it, with `patch` applied on top.
fn backup(patch: Value) -> String {
    let mut base = json!({
        "app": "typio", "schemaVersion": 1, "scope": "all", "exportedAt": "2026-11-06T08:00:00.000Z",
        "fieldRules": [rule()], "suggestionDatasets": datasets()
    });
    for (key, value) in patch.as_object().unwrap() {
        if value.is_null() {
            base.as_object_mut().unwrap().remove(key);
        } else {
            base[key] = value.clone();
        }
    }
    base.to_string()
}

#[test]
fn reads_extension_backups() {
    let parsed = parse_backup(&backup(json!({}))).unwrap();
    assert_eq!(parsed.scope, BackupScope::All);
    assert_eq!(parsed.exported_at, "2026-11-06T08:00:00.000Z");
    assert_eq!(parsed.field_rules, [rule()]);
    assert_eq!(parsed.suggestion_datasets.len(), 2);
    assert_eq!(
        parsed.suggestion_datasets[0].kind(),
        DatasetKind::Values,
        "datasets without a kind are values"
    );
    assert_eq!(
        parsed.suggestion_datasets[1].shortcuts.as_deref(),
        Some(
            &[Shortcut {
                trigger: "pbg".into(),
                expansion: "Phường Bắc Giang".into()
            }][..]
        )
    );

    assert_eq!(
        parse_backup(&format!("\u{feff}{}", backup(json!({})))).unwrap(),
        parsed,
        "a BOM is tolerated"
    );
    assert_eq!(
        parse_backup(&backup(json!({ "scope": null })))
            .unwrap()
            .scope,
        BackupScope::All,
        "backups without a scope are full backups"
    );
}

#[test]
fn rejects_invalid_backups() {
    let error = |text: &str| parse_backup(text).unwrap_err();
    assert_eq!(error("{"), BackupError::InvalidJson);
    assert_eq!(
        error(&backup(json!({ "app": "other" }))),
        BackupError::NotTypio
    );
    assert_eq!(
        error(&backup(json!({ "schemaVersion": 2 }))).to_string(),
        "Unsupported backup version 2. This version of Typio reads version 1."
    );
    assert_eq!(
        error(&backup(json!({ "scope": "profile" }))),
        BackupError::UnknownScope
    );

    let mut bad_rule = rule();
    bad_rule["fingerprint"] = json!({ "tagName": "select", "selector": "x" });
    assert_eq!(
        error(&backup(json!({ "fieldRules": [bad_rule] }))),
        BackupError::InvalidFieldRule
    );

    let mut bad_dataset = datasets()[1].clone();
    bad_dataset["shortcuts"] = json!([{ "trigger": 1 }]);
    assert_eq!(
        error(&backup(json!({ "suggestionDatasets": [bad_dataset] }))),
        BackupError::InvalidDataset
    );

    let first = datasets()[0].clone();
    assert_eq!(
        error(&backup(
            json!({ "suggestionDatasets": [first.clone(), first] })
        )),
        BackupError::DuplicateItems
    );
}

#[test]
fn reads_datasets_backups() {
    let parsed = parse_backup(&backup(json!({ "scope": "datasets", "fieldRules": null }))).unwrap();
    assert_eq!(parsed.scope, BackupScope::Datasets);
    assert!(
        parsed.field_rules.is_empty(),
        "a datasets backup carries no field rules"
    );
    assert_eq!(
        parse_backup(&backup(json!({ "scope": "datasets" }))).unwrap_err(),
        BackupError::UnexpectedFieldRules
    );
}

#[test]
fn dataset_exports_re_import_unchanged() {
    let stamp = || STAMP.to_string();
    let shortcuts = vec![
        Shortcut {
            trigger: "pbg".into(),
            expansion: "Phường Bắc Giang".into(),
        },
        Shortcut {
            trigger: "gio".into(),
            expansion: "Giờ mở cửa: 8:00".into(),
        },
    ];
    let shortcut_set = SuggestionDataset {
        id: "d2".into(),
        name: "Gõ tắt".into(),
        kind: Some(DatasetKind::Shortcuts),
        values: vec![],
        shortcuts: Some(shortcuts.clone()),
        created_at: stamp(),
        updated_at: stamp(),
    };
    assert_eq!(
        parse_shortcuts(&format_dataset_export(&shortcut_set)).shortcuts,
        shortcuts
    );

    let values = vec!["Nguyễn Văn An".to_string(), "Lê Thị Bình".to_string()];
    let value_set = SuggestionDataset {
        kind: None,
        values: values.clone(),
        shortcuts: None,
        ..shortcut_set
    };
    assert_eq!(
        parse_text_suggestions(&format_dataset_export(&value_set)),
        values
    );
}
