use typio_core::{
    build_index, rank_suggestions, IndexedValue, RankedSuggestion, ReplaceScope, SuggestionKind,
};

fn index() -> Vec<IndexedValue> {
    build_index(&[
        "Trần Minh Tuấn",
        "Nguyễn Văn An",
        "Nguyen Van An",
        "Lê Thị Nguyên",
        "Nguyễn Thị Bình",
        "Đặng Đức Dũng",
    ])
}

fn values(index: &[IndexedValue], query: &str, limit: usize) -> Vec<String> {
    rank_suggestions(index, query, limit, ReplaceScope::Field)
        .into_iter()
        .map(|item| item.value)
        .collect()
}

#[test]
fn deduplicates_values_that_normalize_the_same() {
    assert_eq!(index().len(), 5);
}

#[test]
fn ranks_by_kind_then_position() {
    let index = index();
    assert_eq!(
        values(&index, "nguyen", 8),
        ["Nguyễn Văn An", "Nguyễn Thị Bình", "Lê Thị Nguyên"]
    );
    assert_eq!(
        rank_suggestions(&index, "nva", 8, ReplaceScope::Field),
        [RankedSuggestion {
            value: "Nguyễn Văn An".into(),
            kind: SuggestionKind::Initials,
            scope: ReplaceScope::Field,
            trigger: None
        }]
    );
    assert_eq!(values(&index, "nv a", 8), ["Nguyễn Văn An"]);
    assert_eq!(values(&index, "tmt", 8), ["Trần Minh Tuấn"]);
    assert_eq!(values(&index, "dang duc", 8), ["Đặng Đức Dũng"]);
    assert_eq!(values(&index, "nguyen", 1).len(), 1);
}

#[test]
fn accented_queries_prefer_the_exact_spelling() {
    assert_eq!(
        values(&build_index(&["Nguyễn Hà", "Nguyên Hồng"]), "Nguyên", 8),
        ["Nguyên Hồng", "Nguyễn Hà"]
    );
}

#[test]
fn skips_blank_queries_and_the_current_value() {
    let index = index();
    assert!(values(&index, "   ", 8).is_empty());
    assert!(
        values(&index, "Nguyễn Văn An", 8).is_empty(),
        "the value already in the field is not suggested"
    );
}
