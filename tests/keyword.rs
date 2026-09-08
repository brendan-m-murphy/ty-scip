use std::{fs, path::PathBuf, process::Command};

#[test]
fn resolves_cross_module_keyword_argument_to_parameter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/keyword");
    let index = std::env::temp_dir().join(format!("ty-scip-keyword-{}.scip", std::process::id()));

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
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "indexed 2 files: 13 definitions, 15 references; 1 unresolved, 0 ambiguous, \
         0 external, 1 skipped (1 cross-file local, 0 missing symbol)"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(
        stdout
            .lines()
            .any(|line| line == "caller.py:45..51 -> library.py:16..22"),
        "missing cross-file keyword-to-parameter edge\n{stdout}"
    );

    let first = fs::read(&index).expect("read index");
    assert!(!first.is_empty());
    assert!(
        !first
            .windows(b"library/time_offset().(value)".len())
            .any(|window| window == b"library/time_offset().(value)"),
        "lambda parameter must not inherit its enclosing function's global identity"
    );
    let second = run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
