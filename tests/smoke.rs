use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

#[test]
fn resolves_the_four_spike_cases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/smoke/src");
    let index = std::env::temp_dir().join(format!("ty-scip-smoke-{}.scip", std::process::id()));
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
        "indexed 3 files: 16 definitions, 12 references; 0 unresolved, 0 ambiguous, \
         6 external, 0 skipped (0 cross-file local, 0 missing symbol)"
    );
    assert!(output.stdout.is_empty());

    let first = fs::read(&index).expect("read index");
    assert!(!first.is_empty());
    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let main = support::document(&decoded, "src/main.py");
    let library = support::document(&decoded, "src/library.py");
    let definition_write = SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32;
    for (read, definition, definition_roles) in [
        (
            support::occurrence(main, &[8, 11, 20]),
            support::occurrence(main, &[7, 4, 13]),
            definition_write,
        ),
        (
            support::occurrence(main, &[8, 21, 27]),
            support::occurrence(library, &[5, 8, 14]),
            SymbolRole::Definition as i32,
        ),
        (
            support::occurrence(main, &[8, 28, 32]),
            support::occurrence(library, &[0, 4, 10]),
            SymbolRole::Definition as i32,
        ),
        (
            support::occurrence(main, &[8, 33, 40]),
            support::occurrence(main, &[6, 4, 11]),
            definition_write,
        ),
    ] {
        assert_eq!(read.symbol, definition.symbol);
        assert_eq!(read.symbol_roles, SymbolRole::ReadAccess as i32);
        assert_eq!(
            definition.symbol_roles, definition_roles,
            "definition at {:?}",
            definition.range
        );
    }
    assert!(
        first
            .windows(b"ty-scip python . . library/".len())
            .any(|window| window == b"ty-scip python . . library/"),
        "module symbol must use ty's import name, not its src/ path"
    );
    let class_symbol = b"ty-scip python . . library/Formatter#";
    assert!(
        first
            .windows(class_symbol.len())
            .filter(|window| *window == class_symbol)
            .count()
            >= 3,
        "class symbol must have metadata, a definition, and a reference"
    );
    let second_run = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/smoke/src"))
        .arg(&index)
        .output()
        .expect("rerun ty-scip");
    assert!(second_run.status.success());
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
