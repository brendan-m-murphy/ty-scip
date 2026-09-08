use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

#[test]
fn groups_reaching_definitions_by_semantic_place() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/reaching");
    let index = std::env::temp_dir().join(format!("ty-scip-reach-{}.scip", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(&index)
        .output()
        .expect("run ty-scip");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "indexed 1 files: 5 definitions, 3 references; 0 unresolved, 0 ambiguous, \
         0 external, 0 skipped (0 cross-file local, 0 missing symbol)"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(
        stdout
            .lines()
            .any(|line| line == "main.py:97..103 -> main.py:39..45"),
        "the use must resolve to the grouped local binding\n{stdout}"
    );
    for definition in ["main.py:39..45 -> ", "main.py:74..80 -> "] {
        assert!(
            !stdout.lines().any(|line| line.starts_with(definition)),
            "a definition site must not gain a read edge\n{stdout}"
        );
    }

    let decoded = support::read_index(&index);
    let document = support::document(&decoded, "main.py");
    let first = support::occurrence(document, &[2, 8, 14]);
    let second = support::occurrence(document, &[4, 8, 14]);
    let read = support::occurrence(document, &[5, 11, 17]);
    assert_eq!(first.symbol, second.symbol);
    assert_eq!(first.symbol, read.symbol);
    assert!(first.symbol.starts_with("local "));
    assert_eq!(first.symbol_roles, SymbolRole::Definition as i32);
    assert_eq!(second.symbol_roles, SymbolRole::Definition as i32);
    assert_eq!(read.symbol_roles, SymbolRole::ReadAccess as i32);
    fs::remove_file(index).expect("remove test index");
}
