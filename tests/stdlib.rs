use std::{fs, path::PathBuf, process::Command};

use scip::symbol::parse_symbol;

mod support;

#[test]
fn emits_proven_stdlib_symbols_and_omits_typing_only_targets() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/stdlib");
    let output = std::env::temp_dir().join(format!("ty-scip-stdlib-{}.scip", std::process::id()));
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(&output)
        .output()
        .expect("run ty-scip");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    let document = support::document(&index, "main.py");
    let path = index
        .external_symbols
        .iter()
        .find(|symbol| symbol.display_name == "Path")
        .expect("pathlib.Path external symbol");
    let len = index
        .external_symbols
        .iter()
        .find(|symbol| symbol.display_name == "len")
        .expect("builtins.len external symbol");

    for symbol in [path, len] {
        let parsed = parse_symbol(&symbol.symbol).expect("parse stdlib symbol");
        let package = parsed.package.as_ref().expect("stdlib package");
        assert_eq!(package.manager, "python");
        assert_eq!(package.name, "python-stdlib");
        assert_eq!(package.version, "3.12");
    }
    assert_eq!(
        support::occurrence(document, &[3, 7, 11]).symbol,
        path.symbol
    );
    assert_eq!(
        support::occurrence(document, &[4, 7, 10]).symbol,
        len.symbol
    );
    assert!(
        index
            .external_symbols
            .iter()
            .all(|symbol| !symbol.symbol.contains("_typeshed"))
    );
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("7 references; 0 unresolved, 0 ambiguous, 3 external")
    );

    let first = fs::read(&output).expect("read first SCIP index");
    let repeated = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/stdlib"))
        .arg(&output)
        .output()
        .expect("repeat ty-scip");
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(first, fs::read(&output).expect("read repeated SCIP index"));

    fs::remove_file(output).expect("remove SCIP index");
}
