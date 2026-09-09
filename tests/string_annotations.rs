use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

#[test]
fn links_only_analyzer_confirmed_names_inside_string_annotations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/string_annotations");
    let index = std::env::temp_dir().join(format!(
        "ty-scip-string-annotations-{}.scip",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&index)
        .output()
        .expect("run ty-scip");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let document = support::document(&decoded, "main.py");
    let node = document
        .symbols
        .iter()
        .find(|symbol| symbol.display_name == "Node")
        .expect("Node symbol");

    for range in [
        &[12, 13, 17][..],
        &[12, 25, 29],
        &[13, 17, 21],
        &[16, 6, 10],
    ] {
        let occurrence = support::occurrence(document, range);
        assert_eq!(occurrence.symbol, node.symbol, "range {range:?}");
        assert_ne!(
            occurrence.symbol_roles & SymbolRole::Definition as i32,
            SymbolRole::Definition as i32,
            "range {range:?} is a reference"
        );
    }

    for range in [&[14, 20, 24][..], &[14, 30, 34], &[8, 18, 22]] {
        assert!(
            document
                .occurrences
                .iter()
                .all(|occurrence| occurrence.range != range),
            "ordinary string at {range:?} must not become a symbol"
        );
    }

    fs::remove_file(index).expect("remove SCIP index");
}
