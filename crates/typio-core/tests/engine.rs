use typio_core::engine::{
    prefixed_word_at, replacement_for, word_at, Replacement, Suggester, WordRange,
};
use typio_core::sensitive::is_sensitive_label;
use typio_core::{DatasetKind, ReplaceScope, Shortcut, SuggestionDataset, SuggestionKind};

fn dataset(kind: DatasetKind, values: &[&str], shortcuts: &[(&str, &str)]) -> SuggestionDataset {
    SuggestionDataset {
        id: format!("{kind:?}"),
        name: format!("{kind:?}"),
        kind: Some(kind),
        values: values.iter().map(|value| value.to_string()).collect(),
        shortcuts: Some(
            shortcuts
                .iter()
                .map(|(trigger, expansion)| Shortcut {
                    trigger: trigger.to_string(),
                    expansion: expansion.to_string(),
                })
                .collect(),
        ),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn word(text: &str, start: usize, end: usize) -> WordRange {
    WordRange {
        text: text.into(),
        start,
        end,
    }
}

#[test]
fn finds_the_word_before_the_caret() {
    assert_eq!(word_at("Giao tới xtt", 12), Some(word("xtt", 9, 12)));
    assert_eq!(word_at("(xtt", 4), Some(word("xtt", 1, 4)));
    assert_eq!(
        word_at("Đã giao pbg, xong", 11),
        Some(word("pbg", 8, 11)),
        "offsets are UTF-16 units"
    );
    assert_eq!(word_at("xtt abc", 2), None, "a caret inside a word");
    assert_eq!(word_at("xtt ", 4), None, "nothing typed yet");
    assert_eq!(word_at("xtt", 9), None, "a caret past the end");
    assert_eq!(
        word_at("😀 xtt", 6),
        Some(word("xtt", 3, 6)),
        "astral characters count as two units"
    );
}

#[test]
fn finds_the_word_typed_after_a_colon() {
    assert_eq!(prefixed_word_at(":xtt", 4), Some(word("xtt", 0, 4)));
    assert_eq!(
        prefixed_word_at("Giao tới :xtt", 13),
        Some(word("xtt", 9, 13))
    );
    assert_eq!(prefixed_word_at("Giao tới xtt", 12), None, "no colon");
    assert_eq!(prefixed_word_at(":", 1), None, "nothing typed after it");
    assert_eq!(prefixed_word_at("10:30", 5), None, "a time");
    assert_eq!(prefixed_word_at("a:xtt", 5), None, "inside a word");
    assert_eq!(
        prefixed_word_at(":xtt abc", 2),
        None,
        "a caret inside a word"
    );
}

#[test]
fn suggests_values_for_the_field_and_shortcuts_for_the_word() {
    let datasets = [
        dataset(DatasetKind::Values, &["Nguyễn Văn An", "Xã Tam Sơn"], &[]),
        dataset(DatasetKind::Shortcuts, &[], &[("xtt", "Xã Tam Tiến")]),
    ];
    let suggester = Suggester::new(&datasets);

    let field = suggester.suggest("nva", word_at("nva", 3).as_ref());
    assert_eq!(field[0].value, "Nguyễn Văn An");
    assert_eq!(field[0].scope, ReplaceScope::Field);

    let text = "Giao tới xtt";
    let in_text = suggester.suggest(text, word_at(text, 12).as_ref());
    assert_eq!(in_text.len(), 1);
    assert_eq!(in_text[0].kind, SuggestionKind::Shortcut);
    assert_eq!(in_text[0].scope, ReplaceScope::Word);
    assert_eq!(
        replacement_for(text, &in_text[0], word_at(text, 12).as_ref()),
        Replacement {
            start: 9,
            length: 3,
            text: "Xã Tam Tiến".into()
        }
    );

    let alone = suggester.suggest("xtt", word_at("xtt", 3).as_ref());
    assert_eq!(
        alone[0].scope,
        ReplaceScope::Field,
        "a lone trigger replaces the whole field"
    );
    assert_eq!(
        replacement_for("xtt", &alone[0], None),
        Replacement {
            start: 0,
            length: 3,
            text: "Xã Tam Tiến".into()
        }
    );
    assert!(Suggester::new(&[]).is_empty());
}

#[test]
fn flags_sensitive_labels() {
    for label in [
        "Password",
        "Mật khẩu",
        "OTP",
        "Mã xác minh",
        "Card number",
        "API key",
        "Số thẻ",
    ] {
        assert!(is_sensitive_label(label), "{label}");
    }
    for label in ["Họ tên", "Địa chỉ giao hàng", "Ghi chú"] {
        assert!(!is_sensitive_label(label), "{label}");
    }
}
