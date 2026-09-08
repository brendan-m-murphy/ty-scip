use std::{collections::HashSet, fs, path::PathBuf, process::Command};

use scip::types::SymbolRole;

mod support;

fn roles(document: &scip::types::Document, range: &[i32]) -> i32 {
    support::occurrence(document, range).symbol_roles
}

#[test]
fn emits_precise_roles_without_duplicate_occurrences() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/roles");
    let index = std::env::temp_dir().join(format!("ty-scip-roles-{}.scip", std::process::id()));
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
    let library = support::document(&decoded, "library.py");
    let main = support::document(&decoded, "main.py");
    let definition = SymbolRole::Definition as i32;
    let import = SymbolRole::Import as i32;
    let write = SymbolRole::WriteAccess as i32;
    let read = SymbolRole::ReadAccess as i32;

    assert_eq!(roles(library, &[0, 0, 8]), definition | write);
    assert_eq!(roles(library, &[5, 13, 18]), definition | write);

    for range in [[0, 7, 14], [1, 7, 21], [2, 20, 27], [3, 20, 38]] {
        assert_eq!(roles(main, &range), definition | import, "{range:?}");
    }
    for range in [[1, 18, 21], [2, 5, 12], [3, 5, 12], [3, 32, 38]] {
        assert_eq!(roles(main, &range), import, "{range:?}");
    }

    assert_eq!(roles(main, &[5, 0, 5]), definition | write);
    assert_eq!(roles(main, &[6, 0, 5]), definition | read | write);
    assert_eq!(roles(main, &[7, 7, 12]), read);
    assert_eq!(roles(main, &[8, 4, 9]), write);

    assert_eq!(roles(main, &[11, 5, 10]), write);
    assert_eq!(roles(main, &[12, 5, 10]), read | write);
    assert_eq!(roles(main, &[13, 9, 14]), write);
    for range in [[11, 0, 4], [12, 0, 4], [13, 4, 8]] {
        assert_eq!(roles(main, &range), read, "{range:?}");
    }

    assert_eq!(roles(main, &[17, 21, 26]), definition | write);
    assert_eq!(roles(main, &[18, 13, 18]), read);
    assert_eq!(roles(main, &[21, 19, 27]), definition | write);
    assert_eq!(roles(main, &[21, 31, 35]), definition | write);
    assert_eq!(roles(main, &[22, 18, 26]), read);
    assert_eq!(roles(main, &[23, 20, 24]), read);
    assert_eq!(roles(main, &[26, 17, 22]), read);
    assert_eq!(roles(main, &[26, 23, 36]), definition | write);
    assert_eq!(roles(main, &[27, 16, 29]), read);

    for document in [library, main] {
        let mut seen = HashSet::new();
        for occurrence in &document.occurrences {
            assert!(
                seen.insert((occurrence.range.clone(), occurrence.symbol.clone())),
                "duplicate occurrence in {} at {:?}: {}",
                document.relative_path,
                occurrence.range,
                occurrence.symbol
            );
        }
    }

    fs::remove_file(index).expect("remove test index");
}
