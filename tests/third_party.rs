use std::{fs, path::PathBuf, process::Command};

use scip::symbol::parse_symbol;

mod support;

#[test]
fn emits_resolved_third_party_symbols_and_ide_results() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/third_party");
    let output =
        std::env::temp_dir().join(format!("ty-scip-third-party-{}.scip", std::process::id()));
    let facts = output.with_extension("tyfacts");
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .args(["index", root.to_str().unwrap(), "--output"])
        .arg(&output)
        .args(["--facts"])
        .arg(&facts)
        .output()
        .expect("run ty-scip");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    let client = index
        .external_symbols
        .iter()
        .find(|symbol| symbol.display_name == "Client")
        .expect("vendorlib.Client external symbol");
    let run = index
        .external_symbols
        .iter()
        .find(|symbol| symbol.display_name == "run")
        .expect("vendorlib.Client.run external symbol");
    for symbol in [client, run] {
        let parsed = parse_symbol(&symbol.symbol).expect("parse third-party symbol");
        let package = parsed.package.as_ref().expect("third-party package");
        assert_eq!(package.manager, "python");
        assert_eq!(package.name, "vendorlib");
        assert_eq!(package.version, "");
    }
    let document = support::document(&index, "main.py");
    assert!(document.occurrences.iter().any(|occurrence| {
        occurrence.range == [0, 22, 28] && occurrence.symbol == client.symbol
    }));
    assert_eq!(
        support::occurrence(document, &[4, 13, 16]).symbol,
        run.symbol
    );

    let facts_text = fs::read_to_string(&facts).expect("read ty facts");
    assert!(facts_text.contains(&client.symbol));
    assert!(facts_text.contains(&run.symbol));

    fs::remove_file(output).expect("remove SCIP index");
    fs::remove_file(facts).expect("remove ty facts");
}
