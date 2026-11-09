use typio_core::import::{looks_like_shortcuts, parse_shortcuts, parse_suggestion_file};
use typio_core::{
    build_shortcut_index, merge_ranked, rank_suggestions, RankedSuggestion, ReplaceScope, Shortcut,
    SuggestionKind,
};

const SOURCE: &str = "\u{feff}pbg:Phường Bắc Giang\r\nxtt : Xã Tam Tiến\n\nxts:Xã Trường Sơn\nxts2:Xã Tân Sơn\nPBG:Trùng\nnote:Giờ mở cửa: 8:00\nkhông hợp lệ\nx:Một ký tự";

fn shortcut(trigger: &str, expansion: &str) -> Shortcut {
    Shortcut {
        trigger: trigger.into(),
        expansion: expansion.into(),
    }
}

fn word(value: &str, kind: SuggestionKind, trigger: Option<&str>) -> RankedSuggestion {
    RankedSuggestion {
        value: value.into(),
        kind,
        scope: ReplaceScope::Word,
        trigger: trigger.map(Into::into),
    }
}

#[test]
fn parses_shortcut_files() {
    let parsed = parse_shortcuts(SOURCE);
    assert_eq!(
        parsed.shortcuts[..2],
        [
            shortcut("pbg", "Phường Bắc Giang"),
            shortcut("xtt", "Xã Tam Tiến")
        ],
        "BOM is stripped and spaces around ':' are trimmed"
    );
    assert_eq!(parsed.total, 8);
    assert_eq!(
        parsed.duplicates,
        ["PBG"],
        "duplicate triggers are case-insensitive; the first wins"
    );
    assert_eq!(parsed.invalid_lines, [8]);
    assert_eq!(
        parsed
            .shortcuts
            .iter()
            .find(|item| item.trigger == "note")
            .map(|item| item.expansion.as_str()),
        Some("Giờ mở cửa: 8:00"),
        "expansions may contain ':'"
    );
    assert_eq!(parsed.short_triggers, ["x"]);
}

#[test]
fn detects_shortcut_files() {
    assert!(looks_like_shortcuts(
        "\u{feff}pbg:Phường Bắc Giang\nxtt:Xã Tam Tiến\n"
    ));
    assert!(!looks_like_shortcuts("Nguyễn Văn An\nxtt:Xã Tam Tiến"));
    assert!(!looks_like_shortcuts("\n  \n"));
    assert!(!looks_like_shortcuts("a b:c"), "triggers have no spaces");
    assert_eq!(
        parse_suggestion_file("a.txt", "\u{feff}An\nBình").unwrap(),
        ["An", "Bình"],
        "BOM is stripped from value files"
    );
}

#[test]
fn ranks_shortcuts() {
    let index = build_shortcut_index(&parse_shortcuts(SOURCE).shortcuts);
    let rank = |query: &str| rank_suggestions(&index, query, 8, ReplaceScope::Word);

    assert_eq!(
        rank("pbg")[0],
        word("Phường Bắc Giang", SuggestionKind::Shortcut, Some("pbg"))
    );
    assert_eq!(
        rank("xts")
            .into_iter()
            .map(|item| (item.value, item.kind))
            .collect::<Vec<_>>(),
        [
            ("Xã Trường Sơn".to_string(), SuggestionKind::Shortcut),
            ("Xã Tân Sơn".to_string(), SuggestionKind::Initials)
        ],
        "an exact trigger beats an initials match on another expansion"
    );
    assert_eq!(
        rank("XTS2")
            .into_iter()
            .map(|item| item.value)
            .collect::<Vec<_>>(),
        ["Xã Tân Sơn"],
        "triggers match case-insensitively"
    );
    assert_eq!(
        rank("no")[0],
        word("Giờ mở cửa: 8:00", SuggestionKind::Trigger, Some("note")),
        "partial triggers match"
    );
    assert_eq!(
        rank("xt").len(),
        3,
        "a partial trigger lists every expansion starting with those initials"
    );
    assert_eq!(
        rank("bac")[0].value,
        "Phường Bắc Giang",
        "expansions are searchable by text"
    );
}

#[test]
fn merge_keeps_the_best_match_per_value() {
    let merged = merge_ranked(
        vec![
            vec![RankedSuggestion {
                value: "Xã Tân Sơn".into(),
                kind: SuggestionKind::Initials,
                scope: ReplaceScope::Field,
                trigger: None,
            }],
            vec![word("Xã Tân Sơn", SuggestionKind::Shortcut, Some("xts2"))],
        ],
        8,
    );
    assert_eq!(
        merged,
        [word("Xã Tân Sơn", SuggestionKind::Shortcut, Some("xts2"))]
    );
}
