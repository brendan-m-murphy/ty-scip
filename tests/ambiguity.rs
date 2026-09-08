use std::{fs, path::PathBuf, process::Command};

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
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.starts_with("main.py:123..129 -> ")),
        "an ambiguous call must not gain a guessed edge"
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout
            .lines()
            .any(|line| line.starts_with("main.py:82..87 -> ")),
        "a class/function choice must not gain a guessed edge\n{stdout}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line.starts_with("ambiguous main.py:82..87 \"value\" -> ")),
        "the class/function choice must remain ambiguous"
    );
    fs::remove_file(index).expect("remove test index");
}
