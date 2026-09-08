use std::{fs, path::PathBuf, process::Command};

use scip::symbol::parse_symbol;

mod support;

fn run(fixture: &str, label: &str, arguments: &[&str]) -> (PathBuf, Vec<u8>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/project_identity")
        .join(fixture);
    let output = std::env::temp_dir().join(format!(
        "ty-scip-project-identity-{fixture}-{label}-{}.scip",
        std::process::id()
    ));
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .args(arguments)
        .arg(&output)
        .output()
        .expect("run ty-scip");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&output).expect("read SCIP index");
    (output, bytes)
}

#[test]
fn uses_static_pep_621_identity_without_changing_import_names() {
    let (path, _) = run("static", "pep621", &[]);
    let index = support::read_index(&path);
    let provider = support::document(&index, "src/provider.py");
    let consumer = support::document(&index, "src/consumer.py");
    let definition = support::occurrence(provider, &[0, 4, 10]);
    let reference = support::occurrence(consumer, &[2, 9, 15]);

    assert_eq!(definition.symbol, reference.symbol);
    let symbol = parse_symbol(&definition.symbol).expect("parse global symbol");
    let package = symbol.package.as_ref().expect("global symbol package");
    assert_eq!(package.manager, "python");
    assert_eq!(package.name, "example-distribution");
    assert_eq!(package.version, "1.2.3");
    assert_eq!(symbol.descriptors[0].name, "provider");

    fs::remove_file(path).expect("remove SCIP index");
}

#[test]
fn cli_identity_overrides_pep_621_fields_with_positionals() {
    let (path, _) = run(
        "static",
        "override",
        &["--project-name", "override-name", "--project-version=9.8.7"],
    );
    let index = support::read_index(&path);
    let provider = support::document(&index, "src/provider.py");
    let symbol = parse_symbol(&support::occurrence(provider, &[0, 4, 10]).symbol)
        .expect("parse global symbol");
    let package = symbol.package.as_ref().expect("global symbol package");
    assert_eq!(package.name, "override-name");
    assert_eq!(package.version, "9.8.7");

    fs::remove_file(path).expect("remove SCIP index");
}

#[test]
fn missing_dynamic_version_is_empty_and_deterministic() {
    let (path, first) = run("dynamic", "repeat", &[]);
    let index = support::read_index(&path);
    let document = support::document(&index, "main.py");
    let symbol = parse_symbol(&support::occurrence(document, &[0, 0, 6]).symbol)
        .expect("parse global symbol");
    let package = symbol.package.as_ref().expect("global symbol package");
    assert_eq!(package.name, "dynamic-example");
    assert!(package.version.is_empty());

    let (_, second) = run("dynamic", "repeat", &[]);
    assert_eq!(first, second);
    fs::remove_file(path).expect("remove SCIP index");
}
