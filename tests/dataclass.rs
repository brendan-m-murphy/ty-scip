use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

/// A generated dataclass constructor keyword links to its declared field.
#[test]
fn resolves_dataclass_constructor_keywords_deterministically() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/dataclass");
    let output =
        std::env::temp_dir().join(format!("ty-scip-dataclass-{}.scip", std::process::id()));
    let run = || {
        let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
            .arg(&root)
            .arg(&output)
            .output()
            .expect("run ty-scip");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };

    run();
    let first = fs::read(&output).expect("read SCIP index");
    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    let library = support::document(&index, "library.py");
    let caller = support::document(&index, "caller.py");
    let field = support::occurrence(library, &[5, 4, 9]);
    let keyword = support::occurrence(caller, &[2, 7, 12]);

    assert!(field.symbol.contains("library/Record#value."));
    assert_eq!(field.symbol, keyword.symbol);
    assert_eq!(
        field.symbol_roles,
        SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32
    );
    assert_eq!(keyword.symbol_roles, SymbolRole::ReadAccess as i32);

    run();
    assert_eq!(first, fs::read(&output).expect("reread SCIP index"));
    fs::remove_file(output).expect("remove SCIP index");
}
