use std::{fs, path::PathBuf, process::Command};

#[test]
fn resolves_the_four_spike_cases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/smoke/src");
    let index = std::env::temp_dir().join(format!("ty-scip-smoke-{}.scip", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(&index)
        .output()
        .expect("run ty-scip");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");

    for (source, target) in [
        ("main.py:124..133 -> ", "library.py:64..73"),
        ("main.py:157..163 -> ", "library.py:83..89"),
        ("main.py:164..168 -> ", "library.py:4..10"),
        ("main.py:169..176 -> ", "main.py:90..97"),
    ] {
        assert!(
            stdout
                .lines()
                .any(|line| line.contains(source) && line.ends_with(target)),
            "missing {source:?} ... {target:?}\n{stdout}"
        );
    }

    let first = fs::read(&index).expect("read index");
    assert!(!first.is_empty());
    assert!(
        first
            .windows(b"ty-scip python . . library/".len())
            .any(|window| window == b"ty-scip python . . library/"),
        "module symbol must use ty's import name, not its src/ path"
    );
    let class_symbol = b"ty-scip python . . library/Formatter#";
    assert!(
        first
            .windows(class_symbol.len())
            .filter(|window| *window == class_symbol)
            .count()
            >= 3,
        "class symbol must have metadata, a definition, and a reference"
    );
    let second_run = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/smoke/src"))
        .arg(&index)
        .output()
        .expect("rerun ty-scip");
    assert!(second_run.status.success());
    assert_eq!(first, fs::read(&index).expect("reread index"));
    fs::remove_file(index).expect("remove test index");
}
