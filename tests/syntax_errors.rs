use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

#[test]
fn indexes_recovered_syntax_and_reports_parser_diagnostics() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/syntax_errors");
    let index =
        std::env::temp_dir().join(format!("ty-scip-syntax-errors-{}.scip", std::process::id()));
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .lines()
            .any(|line| line == "1 syntax errors, 0 unsupported syntax errors"),
        "{stderr}"
    );
    assert!(output.stdout.is_empty());

    let first = fs::read(&index).expect("read index");
    let decoded = support::read_index(&index);
    let document = support::document(&decoded, "main.py");
    for (reference_range, definition_range) in [
        ([1, 11, 16], [0, 11, 16]),
        ([8, 11, 17], [0, 4, 10]),
        ([8, 18, 23], [7, 10, 15]),
        ([11, 9, 14], [7, 4, 9]),
    ] {
        let reference = support::occurrence(document, &reference_range);
        let definition = support::occurrence(document, &definition_range);
        assert_eq!(reference.symbol, definition.symbol);
        assert_eq!(reference.symbol_roles, SymbolRole::ReadAccess as i32);
        assert_ne!(definition.symbol_roles & SymbolRole::Definition as i32, 0);
    }

    let second = run();
    assert!(second.status.success());
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
