use std::{fs, path::PathBuf, process::Command};

use scip::types::{Occurrence, SymbolRole};

mod support;

fn definition<'a>(document: &'a scip::types::Document, range: &[i32]) -> &'a Occurrence {
    let occurrence = support::occurrence(document, range);
    assert_ne!(
        occurrence.symbol_roles & SymbolRole::Definition as i32,
        0,
        "occurrence at {range:?} is not a definition"
    );
    occurrence
}

#[test]
fn gives_named_nested_definitions_lexical_symbols() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/nested_symbols");
    let index = std::env::temp_dir().join(format!(
        "ty-scip-nested-symbols-{}.scip",
        std::process::id()
    ));
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
    let first_bytes = fs::read(&index).expect("read index");
    let decoded = support::read_index(&index);
    let document = support::document(&decoded, "main.py");

    let first_worker = definition(document, &[1, 8, 14]);
    let first_item = definition(document, &[1, 15, 19]);
    let second_worker = definition(document, &[8, 8, 14]);
    let second_item = definition(document, &[8, 15, 19]);
    assert!(first_worker.symbol.contains("main/first().worker()."));
    assert!(first_item.symbol.contains("main/first().worker().(item)"));
    assert!(second_worker.symbol.contains("main/second().worker()."));
    assert!(second_item.symbol.contains("main/second().worker().(item)"));
    assert_ne!(first_worker.symbol, second_worker.symbol);
    assert_ne!(first_item.symbol, second_item.symbol);
    assert_eq!(
        support::occurrence(document, &[4, 11, 17]).symbol,
        first_worker.symbol
    );
    assert_eq!(
        support::occurrence(document, &[2, 15, 19]).symbol,
        first_item.symbol
    );
    assert_eq!(
        support::occurrence(document, &[11, 11, 17]).symbol,
        second_worker.symbol
    );
    assert_eq!(
        support::occurrence(document, &[9, 15, 19]).symbol,
        second_item.symbol
    );

    let handler = definition(document, &[15, 10, 17]);
    let run_method = definition(document, &[16, 12, 15]);
    let payload = definition(document, &[16, 22, 29]);
    assert!(handler.symbol.contains("main/factory().Handler#"));
    assert!(run_method.symbol.contains("main/factory().Handler#run()."));
    assert!(
        payload
            .symbol
            .contains("main/factory().Handler#run().(payload)")
    );
    assert_eq!(
        support::occurrence(document, &[19, 11, 18]).symbol,
        handler.symbol
    );
    assert_eq!(
        support::occurrence(document, &[17, 19, 26]).symbol,
        payload.symbol
    );

    let selected_then = definition(document, &[24, 12, 20]);
    let selected_else = definition(document, &[27, 12, 20]);
    let selected_parameter_then = definition(document, &[24, 21, 26]);
    let selected_parameter_else = definition(document, &[27, 21, 26]);
    assert!(selected_then.symbol.contains("main/choose().selected()."));
    assert_eq!(selected_then.symbol, selected_else.symbol);
    assert_eq!(
        selected_parameter_then.symbol,
        selected_parameter_else.symbol
    );
    assert!(
        selected_parameter_then
            .symbol
            .contains("main/choose().selected().(value)")
    );
    assert_eq!(
        support::occurrence(document, &[30, 12, 20]).symbol,
        selected_then.symbol
    );
    assert_eq!(
        support::occurrence(document, &[25, 19, 24]).symbol,
        selected_parameter_then.symbol
    );
    assert_eq!(
        support::occurrence(document, &[28, 19, 24]).symbol,
        selected_parameter_then.symbol
    );

    for range in [
        [30, 4, 9],
        [31, 4, 13],
        [31, 23, 28],
        [32, 4, 9],
        [32, 22, 26],
    ] {
        assert!(definition(document, &range).symbol.starts_with("local "));
    }

    let second_output = run();
    assert!(
        second_output.status.success(),
        "{}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    assert_eq!(first_bytes, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
