use std::{fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

/// Generated constructor fields and TypedDict reads link without inventing dict-key symbols.
#[test]
fn links_generated_model_fields_without_treating_plain_dict_keys_as_symbols() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/generated_models");
    let index = std::env::temp_dir().join(format!(
        "ty-scip-generated-models-{}.scip",
        std::process::id()
    ));
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

    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let document = support::document(&decoded, "main.py");

    let movie_title = support::occurrence(document, &[4, 4, 9]);
    let movie_title_keyword = support::occurrence(document, &[13, 14, 19]);
    assert_eq!(movie_title.symbol, movie_title_keyword.symbol);
    assert_eq!(
        movie_title.symbol_roles,
        SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32
    );
    assert_eq!(
        movie_title_keyword.symbol_roles,
        SymbolRole::ReadAccess as i32
    );

    let point_x = support::occurrence(document, &[9, 4, 5]);
    let point_x_keyword = support::occurrence(document, &[19, 14, 15]);
    assert_eq!(point_x.symbol, point_x_keyword.symbol);
    assert_eq!(point_x_keyword.symbol_roles, SymbolRole::ReadAccess as i32);

    let movie_title_subscript = support::occurrence(document, &[14, 24, 29]);
    assert_eq!(movie_title_subscript.symbol, movie_title.symbol);
    assert_eq!(
        movie_title_subscript.symbol_roles,
        SymbolRole::ReadAccess as i32
    );

    let ordinary_key = support::occurrence(document, &[16, 12, 19]);
    assert!(scip::symbol::is_local_symbol(&ordinary_key.symbol));
    assert_ne!(ordinary_key.symbol, movie_title.symbol);
    assert!(
        !document
            .occurrences
            .iter()
            .any(|occurrence| occurrence.range == [17, 30, 35])
    );

    let first = fs::read(&index).expect("read test index");
    let repeated = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/generated_models"))
        .arg(&index)
        .output()
        .expect("rerun ty-scip");
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(first, fs::read(&index).expect("reread test index"));

    fs::remove_file(index).expect("remove test index");
}
