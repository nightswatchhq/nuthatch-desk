//! The threat model's first row, held to the source: no string from a nest reaches a rich-text
//! path. `live.rs` proves the window does not fetch a hostile nest's markup; this keeps the next
//! label somebody adds from being the one that does.

use std::{fs, path::Path};

fn qml_files() -> Vec<(String, String)> {
    // The test lives in this crate because it needs no Qt, and a test in `desk` has to link it.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../desk/qml");
    let mut files: Vec<(String, String)> = fs::read_dir(dir)
        .expect("the qml directory")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "qml"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            (name, fs::read_to_string(&path).expect("a readable file"))
        })
        .collect();
    files.sort();
    files
}

/// Whether `line` creates an object of type `name`, as `Name {` or `property: Name {`.
fn creates(line: &str, name: &str) -> bool {
    let line = line.trim();
    let opening = format!("{name} {{");
    line.starts_with(&opening) || line.contains(&format!(": {opening}"))
}

#[test]
fn every_label_is_a_plain_label() {
    let files = qml_files();
    assert!(files.len() >= 9, "found only {} QML files", files.len());
    for (name, text) in &files {
        for (number, line) in text.lines().enumerate() {
            // `PlainLabel.qml` is the one place a `Label` may be made, and it fixes the format.
            let allowed = name == "PlainLabel.qml";
            for raw in ["Text", "Label"] {
                assert!(
                    allowed || !creates(line, raw),
                    "{name}:{}: a bare {raw} renders markup; use PlainLabel or Mono",
                    number + 1
                );
            }
        }
    }
    let plain = &files
        .iter()
        .find(|(name, _)| name == "PlainLabel.qml")
        .expect("PlainLabel.qml")
        .1;
    assert!(plain.contains("textFormat: Text.PlainText"));
}

#[test]
fn every_text_editor_is_plain() {
    for (name, text) in qml_files() {
        let editors = text
            .lines()
            .filter(|line| creates(line, "TextEdit") || creates(line, "TextArea"))
            .count();
        let plain = text.matches("textFormat: TextEdit.PlainText").count();
        assert_eq!(
            editors, plain,
            "{name}: an editor without a plain text format"
        );
    }
}

#[test]
fn the_rule_sees_what_it_is_looking_for() {
    assert!(creates("    Label {", "Label"));
    assert!(creates("contentItem: Text {", "Text"));
    assert!(!creates("    PlainLabel {", "Label"));
    assert!(!creates("    TextArea {", "Text"));
    assert!(!creates("    // a Label { in a comment", "Label"));
}
