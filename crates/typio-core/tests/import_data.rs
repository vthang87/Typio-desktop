use typio_core::import::{
    parse_csv_suggestions, parse_json_suggestions, parse_suggestion_file, parse_text_suggestions,
};

#[test]
fn parses_text() {
    assert_eq!(parse_text_suggestions(" An \nBình\nAn\n\n"), ["An", "Bình"]);
}

#[test]
fn parses_csv_with_quotes() {
    assert_eq!(
        parse_csv_suggestions("\"Nguyễn, An\",Bình\r\nChâu,\"Dũng \""),
        ["Nguyễn, An", "Bình", "Châu", "Dũng"]
    );
    assert_eq!(parse_csv_suggestions("\"Say \"\"hi\"\"\""), ["Say \"hi\""]);
}

#[test]
fn parses_json_in_document_order() {
    assert_eq!(
        parse_json_suggestions(r#"["An", {"name":"Bình", "score": 3}, ["An"]]"#).unwrap(),
        ["An", "Bình", "3"]
    );
    assert_eq!(
        parse_json_suggestions(r#"{"z": "Z", "a": 1.5, "b": 2.0}"#).unwrap(),
        ["Z", "1.5", "2"]
    );
    assert!(parse_json_suggestions("not-json").is_err());
}

#[test]
fn picks_the_parser_by_extension() {
    assert_eq!(
        parse_suggestion_file("names.TXT", "An\nBình").unwrap(),
        ["An", "Bình"]
    );
    assert_eq!(
        parse_suggestion_file("names.csv", "An,Bình").unwrap(),
        ["An", "Bình"]
    );
    assert_eq!(
        parse_suggestion_file("names.json", "\u{feff}[\"An\"]").unwrap(),
        ["An"]
    );
}
