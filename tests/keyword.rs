use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

#[test]
fn resolves_cross_module_keyword_arguments_to_parameters() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/keyword");
    let index = std::env::temp_dir().join(format!("ty-scip-keyword-{}.scip", std::process::id()));

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_ty-scip"))
            .arg(&root)
            .arg(&index)
            .output()
            .expect("run ty-scip")
    };

    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "indexed 2 files: 26 definitions, 29 references; 1 unresolved, 0 ambiguous, \
         0 external, 0 skipped (0 cross-file local, 0 missing symbol)"
    );
    assert!(output.stdout.is_empty());

    let first = fs::read(&index).expect("read index");
    assert!(!first.is_empty());
    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let caller = support::document(&decoded, "caller.py");
    let library = support::document(&decoded, "library.py");
    assert!(
        !library
            .symbols
            .iter()
            .any(|symbol| symbol.symbol.contains("library/time_offset().(value)")),
        "lambda parameter must not inherit its enclosing function's global identity"
    );
    let period_call = support::occurrence(caller, &[2, 12, 18]);
    let period_definition = support::occurrence(library, &[3, 16, 22]);
    assert_eq!(period_call.symbol, period_definition.symbol);
    assert_eq!(period_call.symbol_roles, SymbolRole::ReadAccess as i32);
    assert_eq!(
        period_definition.symbol_roles,
        SymbolRole::Definition as i32
    );
    let call = support::occurrence(caller, &[3, 17, 23]);
    let overloads =
        [[9, 23, 29], [13, 23, 29], [16, 18, 24]].map(|range| support::occurrence(library, &range));
    assert!(call.symbol.contains("library/process().(format)"));
    assert_eq!(call.symbol_roles, SymbolRole::ReadAccess as i32);
    for definition in overloads {
        assert_eq!(definition.symbol, call.symbol);
        assert_eq!(definition.symbol_roles, SymbolRole::Definition as i32);
    }
    let second = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&index)
        .env("TY_SCIP_SAMPLE_LIMIT", "1")
        .output()
        .expect("rerun ty-scip with diagnostic samples");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr)
            .lines()
            .any(|line| line == "unresolved caller.py:147..154 \"unknown\""),
        "missing deterministic unresolved sample"
    );
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
