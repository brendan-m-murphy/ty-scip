use std::{fs, path::PathBuf, process::Command};

#[cfg(unix)]
use std::{
    os::unix::fs::symlink,
    time::{SystemTime, UNIX_EPOCH},
};

use scip::symbol::parse_symbol;

mod support;

fn run(label: &str) -> (PathBuf, Vec<u8>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/project_discovery");
    let output = std::env::temp_dir().join(format!(
        "ty-scip-project-discovery-{label}-{}.scip",
        std::process::id()
    ));
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
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

/// Exclude configured Python files from the emitted documents.
#[test]
fn honors_pyproject_excludes_deterministically() {
    let (path, first) = run("exclude-first");
    let index = support::read_index(&path);
    support::assert_index_integrity(&index);

    let paths: Vec<_> = index
        .documents
        .iter()
        .map(|document| document.relative_path.as_str())
        .collect();
    assert_eq!(paths, ["consumer.py", "namespace/api.py"]);

    let (second_path, second) = run("exclude-second");
    assert_eq!(first, second);
    fs::remove_file(path).expect("remove SCIP index");
    fs::remove_file(second_path).expect("remove repeated SCIP index");
}

/// Give namespace-package definitions and references one durable identity.
#[test]
fn links_pep_420_namespace_package_symbols() {
    let (path, _) = run("namespace");
    let index = support::read_index(&path);
    support::assert_index_integrity(&index);
    let provider = support::document(&index, "namespace/api.py");
    let consumer = support::document(&index, "consumer.py");
    let definition = support::occurrence(provider, &[0, 4, 12]);
    let imported = consumer
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.range == [0, 26, 34] && occurrence.symbol == definition.symbol
        })
        .expect("namespace import target");
    let called = consumer
        .occurrences
        .iter()
        .find(|occurrence| occurrence.range == [2, 9, 17] && occurrence.symbol == definition.symbol)
        .expect("namespace call target");

    assert_eq!(definition.symbol, imported.symbol);
    assert_eq!(definition.symbol, called.symbol);
    let symbol = parse_symbol(&definition.symbol).expect("parse global symbol");
    let descriptor_names: Vec<_> = symbol
        .descriptors
        .iter()
        .map(|descriptor| descriptor.name.as_str())
        .collect();
    assert_eq!(descriptor_names, ["namespace", "api", "exported"]);

    fs::remove_file(path).expect("remove SCIP index");
}

/// Match ty's policy of selecting file symlinks without traversing directory symlinks.
#[cfg(unix)]
#[test]
fn handles_project_symlinks_like_ty() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "ty-scip-project-discovery-symlinks-{}-{nonce}",
        std::process::id()
    ));
    let root = temporary.join("project");
    let linked_directory = temporary.join("linked-directory");
    fs::create_dir_all(&root).expect("create project");
    fs::create_dir(&linked_directory).expect("create linked directory");
    fs::write(temporary.join("linked.py"), "linked = True\n").expect("write linked file");
    fs::write(linked_directory.join("ignored.py"), "ignored = True\n").expect("write ignored file");
    symlink(temporary.join("linked.py"), root.join("linked.py")).expect("link Python file");
    symlink(&linked_directory, root.join("linked-directory")).expect("link directory");

    let output = temporary.join("index.scip");
    let result = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&output)
        .output()
        .expect("run ty-scip");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let index = support::read_index(&output);
    support::assert_index_integrity(&index);
    let paths = index
        .documents
        .iter()
        .map(|document| document.relative_path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["linked.py"]);

    fs::remove_dir_all(temporary).expect("remove temporary project");
}
