use std::{fs, path::PathBuf, process::Command};

use scip::types::{Relationship, SymbolInformation, SymbolRole, symbol_information};

mod support;

fn symbol<'a>(document: &'a scip::types::Document, name: &str) -> &'a SymbolInformation {
    document
        .symbols
        .iter()
        .find(|symbol| symbol.display_name == name)
        .unwrap_or_else(|| panic!("missing symbol metadata for {name}"))
}

fn relationship_targets(
    symbol: &SymbolInformation,
    predicate: impl Fn(&Relationship) -> bool,
) -> Vec<&str> {
    symbol
        .relationships
        .iter()
        .filter(|relationship| predicate(relationship))
        .map(|relationship| relationship.symbol.as_str())
        .collect()
}

#[test]
fn emits_internal_class_implementation_relationships() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/relationships");
    let index =
        std::env::temp_dir().join(format!("ty-scip-relationships-{}.scip", std::process::id()));
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
    let base = support::document(&decoded, "fixture/base.py");
    let models = support::document(&decoded, "fixture/models.py");
    let base_symbol = &symbol(base, "Base").symbol;
    let left_symbol = &symbol(base, "Left").symbol;
    let right_symbol = &symbol(base, "Right").symbol;
    let implementations = |name| {
        relationship_targets(symbol(models, name), |relationship| {
            relationship.is_implementation
        })
    };

    assert_eq!(implementations("Child"), [base_symbol.as_str()]);
    assert_eq!(
        implementations("Mixed"),
        [left_symbol.as_str(), right_symbol.as_str()]
    );
    assert_eq!(implementations("WithMeta"), [base_symbol.as_str()]);
    assert!(implementations("Meta").is_empty());

    let base_reference = support::occurrence(models, &[3, 12, 16]);
    assert_eq!(base_reference.symbol, *base_symbol);
    assert_eq!(base_reference.symbol_roles, SymbolRole::ReadAccess as i32);

    for related in models
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.relationships)
    {
        assert!(related.is_implementation);
        assert!(!related.is_type_definition);
        assert!(!related.is_reference);
        assert!(!related.is_definition);
    }
    assert_eq!(
        symbol(models, "Child").kind.enum_value().unwrap(),
        symbol_information::Kind::Class
    );

    fs::remove_file(index).expect("remove test index");
}
