use std::{fs, path::PathBuf, process::Command};

mod support;

#[test]
/// Verifies unnameable paths are skipped with warnings instead of aborting the run,
/// and that the surviving documents keep their positional symbol links intact.
fn skips_unimportable_paths_and_keeps_symbol_links_aligned() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/unimportable_paths");
    let index = std::env::temp_dir().join(format!(
        "ty-scip-unimportable-paths-{}.scip",
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.stdout.is_empty());

    // Each unnameable path is skipped with a warning that names it: a dotted
    // directory sorting first, a hyphenated directory, and a hyphenated file.
    for skipped in [
        ".aaa_pkg/early.py",
        "data-pipeline.py",
        "tools-dev/helper.py",
    ] {
        let suffix = format!("{skipped} (path cannot form an importable module name)");
        assert!(
            stderr
                .lines()
                .any(|line| line.starts_with("warning: skipping ") && line.ends_with(&suffix)),
            "missing skip warning for {skipped}: {stderr}"
        );
    }

    let first = fs::read(&index).expect("read SCIP index");
    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    assert_eq!(
        decoded
            .documents
            .iter()
            .map(|document| document.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["app/core.py", "consumer.py"],
        "only the importable files belong in the index"
    );
    for skipped in [
        ".aaa_pkg/early.py",
        "data-pipeline.py",
        "tools-dev/helper.py",
    ] {
        assert!(
            !decoded
                .documents
                .iter()
                .any(|document| document.relative_path == skipped),
            "{skipped} must not appear in the index"
        );
    }

    // The skipped `.aaa_pkg/early.py` sorted before every included file, so any
    // misalignment between `file_indices` and `data` would shift these
    // positional lookups and resolve `render` against the wrong file.
    let consumer = support::document(&decoded, "consumer.py");
    let core = support::document(&decoded, "app/core.py");
    let called_render = support::occurrence(consumer, &[2, 0, 6]);
    let rendered_definition = support::occurrence(core, &[0, 4, 10]);
    assert_eq!(called_render.symbol, rendered_definition.symbol);

    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first, fs::read(&index).expect("reread SCIP index"));
    fs::remove_file(&index).expect("remove SCIP index");
}

#[test]
/// Verifies --quiet suppresses the unimportable-path skip warnings, which are
/// indexing diagnostics routed through the caller's gate, while the run still
/// succeeds and still writes the index.
fn quiet_suppresses_unimportable_path_warnings() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/unimportable_paths");
    let index = std::env::temp_dir().join(format!(
        "ty-scip-unimportable-paths-quiet-{}.scip",
        std::process::id()
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg("--quiet")
        .arg(&root)
        .arg(&index)
        .output()
        .expect("run ty-scip");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.is_empty(),
        "--quiet must suppress every indexing diagnostic, including skip warnings: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let decoded = support::read_index(&index);
    support::assert_index_integrity(&decoded);
    assert_eq!(
        decoded
            .documents
            .iter()
            .map(|document| document.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["app/core.py", "consumer.py"],
        "--quiet must not change what is indexed"
    );
    fs::remove_file(&index).expect("remove SCIP index");
}
