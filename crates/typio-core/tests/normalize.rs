use typio_core::{get_initials, get_match_kind, normalize_vietnamese, MatchKind};

#[test]
fn normalizes_vietnamese() {
    assert_eq!(normalize_vietnamese("  Nguyễn Đình  An "), "nguyen dinh an");
    assert_eq!(
        normalize_vietnamese("Phường Bắc Giang!"),
        "phuong bac giang"
    );
    assert_eq!(normalize_vietnamese("  --  "), "");
}

#[test]
fn builds_initials() {
    assert_eq!(get_initials("Nguyễn Văn An"), "nva");
    assert_eq!(get_initials("Trần-Minh  Tuấn"), "tmt");
}

#[test]
fn classifies_matches() {
    assert_eq!(
        get_match_kind("Nguyễn Văn An", "nguyen van an"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        get_match_kind("Nguyễn Văn An", "nguyen"),
        Some(MatchKind::Prefix)
    );
    assert_eq!(
        get_match_kind("Nguyễn Văn An", "nva"),
        Some(MatchKind::Initials)
    );
    assert_eq!(
        get_match_kind("Nguyễn Văn An", "van an"),
        Some(MatchKind::Contains)
    );
    assert_eq!(get_match_kind("Nguyễn Văn An", "xyz"), None);
}
