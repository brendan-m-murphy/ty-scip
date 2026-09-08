use std::{fs, path::PathBuf, process::Command};

#[test]
fn resolves_cross_module_keyword_arguments_to_parameters() {
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
        "indexed 2 files: 22 definitions, 19 references; 1 unresolved, 0 ambiguous, \
         10 external, 1 skipped (1 cross-file local, 0 missing symbol)"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(
        stdout
            .lines()
            .any(|line| line == "caller.py:54..60 -> library.py:46..52"),
        "missing cross-file keyword-to-parameter edge\n{stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line == "caller.py:84..90 -> library.py:160..166"),
        "missing overloaded keyword-to-parameter edge\n{stdout}"
    );
    for definition in ["160..166", "220..226", "265..271"] {
        assert!(
            !stdout
                .lines()
                .any(|line| line.starts_with(&format!("library.py:{definition} ->"))),
            "overload definition was also emitted as a read\n{stdout}"
        );
    }

    let first = fs::read(&index).expect("read index");
    assert!(!first.is_empty());
    assert!(
        !first
            .windows(b"library/time_offset().(value)".len())
            .any(|window| window == b"library/time_offset().(value)"),
        "lambda parameter must not inherit its enclosing function's global identity"
    );
    assert!(
        first
            .windows(b"library/process().(format)".len())
            .any(|window| window == b"library/process().(format)"),
        "overload parameters must share one durable symbol"
    );
    let second = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&index)
        .env("TY_SCIP_SAMPLE_LIMIT", "1")
        .output()
        .expect("rerun ty-scip with diagnostic samples");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr)
            .lines()
            .any(|line| line == "unresolved caller.py:147..154 \"unknown\""),
        "missing deterministic unresolved sample"
    );
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
