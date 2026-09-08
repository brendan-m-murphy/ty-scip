use std::{fs, path::PathBuf, process::Command};

mod support;

#[test]
/// Verifies stubs drive references while source and stub documents share identities.
fn indexes_stubbed_modules_with_one_stable_symbol_identity() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/stubs");
    let output = std::env::temp_dir().join(format!("ty-scip-stubs-{}.scip", std::process::id()));
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_ty-scip"))
            .arg(&root)
            .arg(&output)
            .output()
            .expect("run ty-scip")
    };

    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let first = fs::read(&output).expect("read SCIP index");
    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    assert_eq!(
        index
            .documents
            .iter()
            .map(|document| document.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["consumer.py", "library.py", "library.pyi", "stub_only.pyi"]
    );

    let consumer = support::document(&index, "consumer.py");
    let implementation = support::document(&index, "library.py");
    let stub = support::document(&index, "library.pyi");
    let stub_only = support::document(&index, "stub_only.pyi");

    let called_render = support::occurrence(consumer, &[3, 0, 6]);
    let implementation_render = support::occurrence(implementation, &[0, 4, 10]);
    let stub_render = support::occurrence(stub, &[0, 4, 10]);
    assert!(consumer.occurrences.iter().any(|occurrence| {
        occurrence.range == [0, 20, 26] && occurrence.symbol == called_render.symbol
    }));
    assert_eq!(called_render.symbol, stub_render.symbol);
    assert_eq!(stub_render.symbol, implementation_render.symbol);

    let style_reference = support::occurrence(consumer, &[3, 16, 21]);
    let style_parameter = support::occurrence(stub, &[0, 23, 28]);
    assert_eq!(style_reference.symbol, style_parameter.symbol);
    assert!(
        !implementation
            .occurrences
            .iter()
            .any(|occurrence| occurrence.symbol == style_reference.symbol),
        "the keyword is provided by the selected stub, not the implementation signature"
    );

    let called_parse = support::occurrence(consumer, &[4, 0, 5]);
    let parse_definition = support::occurrence(stub_only, &[0, 4, 9]);
    let strict_reference = support::occurrence(consumer, &[4, 11, 17]);
    let strict_parameter = support::occurrence(stub_only, &[0, 26, 32]);
    assert!(consumer.occurrences.iter().any(|occurrence| {
        occurrence.range == [1, 22, 27] && occurrence.symbol == parse_definition.symbol
    }));
    assert_eq!(called_parse.symbol, parse_definition.symbol);
    assert_eq!(strict_reference.symbol, strict_parameter.symbol);

    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first, fs::read(&output).expect("reread SCIP index"));
    fs::remove_file(output).expect("remove SCIP index");
}
