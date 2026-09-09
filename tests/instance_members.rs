use std::{fs, path::PathBuf, process::Command};

use scip::types::{Occurrence, SymbolRole};

mod support;

fn occurrences_for<'a>(
    documents: impl IntoIterator<Item = &'a scip::types::Document>,
    symbol: &'a str,
) -> Vec<&'a Occurrence> {
    documents
        .into_iter()
        .flat_map(|document| &document.occurrences)
        .filter(|occurrence| occurrence.symbol == symbol)
        .collect()
}

#[test]
fn promotes_only_proven_instance_attributes_to_class_members() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/instance_members");
    let index = std::env::temp_dir().join(format!("ty-scip-instance-{}.scip", std::process::id()));
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
    let caller = support::document(&decoded, "caller.py");
    let library = support::document(&decoded, "library.py");

    let first = support::occurrence(library, &[18, 13, 18]);
    let repeated = support::occurrence(library, &[21, 17, 22]);
    let same_file_read = support::occurrence(library, &[24, 20, 25]);
    let cross_file_read = support::occurrence(caller, &[4, 8, 13]);
    let inherited_read = support::occurrence(caller, &[9, 20, 25]);
    let class_variable = support::occurrence(library, &[15, 4, 9]);
    assert!(first.symbol.contains("Counter#value."));
    for occurrence in [
        repeated,
        same_file_read,
        cross_file_read,
        inherited_read,
        class_variable,
    ] {
        assert_eq!(occurrence.symbol, first.symbol);
    }
    assert_eq!(
        first.symbol_roles,
        SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32
    );
    assert_eq!(first.enclosing_range, [18, 8, 18]);
    assert_eq!(
        repeated.symbol_roles,
        SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32
    );
    assert_eq!(same_file_read.symbol_roles, SymbolRole::ReadAccess as i32);
    assert_eq!(cross_file_read.symbol_roles, SymbolRole::ReadAccess as i32);
    assert_eq!(inherited_read.symbol_roles, SymbolRole::ReadAccess as i32);

    let property = support::occurrence(library, &[32, 8, 13]);
    let property_assignment = support::occurrence(library, &[37, 13, 18]);
    let property_read = support::occurrence(caller, &[13, 9, 14]);
    assert!(
        property.symbol.contains("Labelled#label"),
        "{}",
        property.symbol
    );
    assert_eq!(property_assignment.symbol, property.symbol);
    assert_eq!(property_read.symbol, property.symbol);

    let all_occurrences = occurrences_for([library, caller], &first.symbol);
    assert_eq!(
        all_occurrences
            .iter()
            .filter(|occurrence| { occurrence.symbol_roles & SymbolRole::Definition as i32 != 0 })
            .count(),
        3
    );
    for range in [[27, 14, 21], [43, 15, 26], [47, 15, 25], [51, 15, 28]] {
        assert!(
            !library
                .occurrences
                .iter()
                .any(|occurrence| occurrence.range == range)
        );
    }
    assert!(!library.symbols.iter().any(|symbol| {
        [
            "foreign",
            "property_only",
            "static_only",
            "class_only",
            "replaced_only",
        ]
        .iter()
        .any(|name| symbol.symbol.contains(name))
    }));
    assert!(
        support::occurrence(library, &[33, 13, 26])
            .symbol
            .starts_with("local ")
    );
    assert!(
        !caller
            .occurrences
            .iter()
            .any(|occurrence| occurrence.range == [17, 16, 21])
    );
    let other = support::occurrence(library, &[56, 13, 18]);
    assert!(other.symbol.contains("Other#value."));
    assert_ne!(other.symbol, first.symbol);

    let decorated_assignment = support::occurrence(library, &[62, 13, 16]);
    let decorated_read = support::occurrence(caller, &[21, 10, 13]);
    assert!(decorated_assignment.symbol.contains("Decorated#tag."));
    assert_eq!(decorated_read.symbol, decorated_assignment.symbol);
    assert_eq!(
        decorated_assignment.symbol_roles,
        SymbolRole::Definition as i32 | SymbolRole::WriteAccess as i32
    );
    assert_eq!(decorated_read.symbol_roles, SymbolRole::ReadAccess as i32);

    fs::remove_file(index).expect("remove test index");
}
