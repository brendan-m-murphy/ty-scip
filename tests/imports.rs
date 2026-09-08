use std::{fs, path::PathBuf, process::Command};

use scip::types::{Occurrence, SymbolRole};

mod support;

fn run(root: &PathBuf, output: &PathBuf) {
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(output)
        .output()
        .expect("run ty-scip");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn occurrence_with_symbol<'a>(
    document: &'a scip::types::Document,
    range: &[i32],
    symbol: &str,
) -> &'a Occurrence {
    document
        .occurrences
        .iter()
        .find(|occurrence| occurrence.range == range && occurrence.symbol == symbol)
        .unwrap_or_else(|| panic!("missing occurrence {range:?} for {symbol}"))
}

/// Imports and re-exports retain the original definition identity without false links.
#[test]
fn resolves_package_import_forms_and_reexports_deterministically() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/imports");
    let output = std::env::temp_dir().join(format!("ty-scip-imports-{}.scip", std::process::id()));
    run(&root, &output);
    let first = fs::read(&output).expect("read SCIP index");
    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    let consumer = support::document(&index, "consumer.py");
    let api = support::document(&index, "pkg/api.py");
    let package = support::document(&index, "pkg/__init__.py");
    let module = support::document(&index, "pkg/deep/module.py");
    let other = support::document(&index, "other.py");

    let target = support::occurrence(module, &[0, 4, 10]);
    assert_eq!(target.symbol_roles, SymbolRole::Definition as i32);

    for (document, range) in [
        (api, [0, 25, 31]),
        (api, [0, 35, 42]),
        (package, [0, 25, 31]),
        (consumer, [3, 28, 34]),
        (consumer, [3, 38, 52]),
        (consumer, [4, 16, 22]),
    ] {
        let occurrence = occurrence_with_symbol(document, &range, &target.symbol);
        assert_eq!(
            occurrence.symbol_roles,
            SymbolRole::Import as i32,
            "{range:?}"
        );
    }
    assert_eq!(
        occurrence_with_symbol(api, &[4, 11, 18], &target.symbol).symbol_roles,
        SymbolRole::ReadAccess as i32
    );
    for range in [
        [7, 15, 21],
        [8, 12, 18],
        [9, 7, 13],
        [10, 0, 14],
        [11, 0, 6],
    ] {
        let occurrence = occurrence_with_symbol(consumer, &range, &target.symbol);
        assert_eq!(
            occurrence.symbol_roles,
            SymbolRole::ReadAccess as i32,
            "{range:?}"
        );
    }

    let module_symbol = &support::occurrence(module, &[0, 0, 0]).symbol;
    for range in [[0, 26, 40], [2, 21, 27]] {
        let occurrence = occurrence_with_symbol(consumer, &range, module_symbol);
        assert_eq!(
            occurrence.symbol_roles,
            SymbolRole::Import as i32,
            "{range:?}"
        );
    }
    for range in [[7, 0, 14], [9, 0, 6]] {
        assert_eq!(support::occurrence(consumer, &range).symbol, *module_symbol);
    }

    let other_target = support::occurrence(other, &[0, 4, 10]);
    assert_ne!(target.symbol, other_target.symbol);
    assert_eq!(
        support::occurrence(consumer, &[12, 6, 12]).symbol,
        other_target.symbol
    );

    run(&root, &output);
    assert_eq!(first, fs::read(&output).expect("reread SCIP index"));
    fs::remove_file(output).expect("remove SCIP index");
}
