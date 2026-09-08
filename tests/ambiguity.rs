use std::{fs, path::PathBuf, process::Command};

#[allow(dead_code)]
mod support;

#[test]
fn keeps_distinct_semantic_places_ambiguous() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/ambiguity");
    let index = std::env::temp_dir().join(format!("ty-scip-ambiguity-{}.scip", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(&index)
        .env("TY_SCIP_SAMPLE_LIMIT", "1")
        .output()
        .expect("run ty-scip");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.lines().any(|line| {
            line.starts_with("ambiguous main.py:123..129 \"choice\" -> ")
                && line.contains("main.py:4..8=ty-scip python . . main/left().")
                && line.contains("main.py:79..85=?")
        }),
        "missing distinct-place sample\n{stderr}"
    );
    assert!(output.stdout.is_empty());
    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let document = support::document(&decoded, "main.py");
    assert!(
        document
            .occurrences
            .iter()
            .all(|occurrence| occurrence.range != [10, 11, 17]),
        "an ambiguous call must not gain a guessed occurrence"
    );
    fs::remove_file(index).expect("remove test index");
}

#[test]
fn keeps_distinct_global_symbol_kinds_ambiguous() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed_kind");
    let index =
        std::env::temp_dir().join(format!("ty-scip-mixed-kind-{}.scip", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(&index)
        .env("TY_SCIP_SAMPLE_LIMIT", "2")
        .output()
        .expect("run ty-scip");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    let document = support::document(&decoded, "main.py");
    assert!(
        document
            .occurrences
            .iter()
            .all(|occurrence| occurrence.range != [8, 0, 5]),
        "a class/function choice must not gain a guessed occurrence"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line.starts_with("ambiguous main.py:82..87 \"value\" -> ")),
        "the class/function choice must remain ambiguous"
    );
    fs::remove_file(index).expect("remove test index");
}
