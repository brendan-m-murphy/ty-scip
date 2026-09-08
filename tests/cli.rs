use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn project(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("ty-scip-cli-{name}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&directory).expect("create temporary project");
    fs::write(directory.join("main.py"), "answer = 42\n").expect("write Python source");
    directory
}

#[test]
fn supports_public_command_line_conventions() {
    let binary = env!("CARGO_BIN_EXE_ty-scip");

    let help = Command::new(binary).arg("--help").output().expect("help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage: ty-scip"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--project-name NAME"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--project-version VERSION"));
    assert!(help.stderr.is_empty());

    let version = Command::new(binary)
        .arg("--version")
        .output()
        .expect("version");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        concat!("ty-scip ", env!("CARGO_PKG_VERSION"))
    );
    assert!(version.stderr.is_empty());

    let zero_argument_project = project("zero");
    let indexed = Command::new(binary)
        .current_dir(&zero_argument_project)
        .output()
        .expect("index current directory");
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    assert!(zero_argument_project.join("index.scip").is_file());
    assert!(indexed.stdout.is_empty());

    let caller = project("caller");
    let target = project("target");
    let indexed = Command::new(binary)
        .current_dir(&caller)
        .arg(&target)
        .output()
        .expect("index positional project root");
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    assert!(caller.join("index.scip").is_file());

    let invalid = Command::new(binary)
        .arg("--unknown")
        .output()
        .expect("reject unknown option");
    assert!(!invalid.status.success());
    assert_eq!(
        String::from_utf8_lossy(&invalid.stderr).trim(),
        "ty-scip: unknown option --unknown; try `ty-scip --help`"
    );

    let missing_value = Command::new(binary)
        .arg("--project-name")
        .output()
        .expect("reject missing option value");
    assert!(!missing_value.status.success());
    assert_eq!(
        String::from_utf8_lossy(&missing_value.stderr).trim(),
        "ty-scip: --project-name requires a value"
    );

    let extra = Command::new(binary)
        .args(["one", "two", "three"])
        .output()
        .expect("reject extra arguments");
    assert!(!extra.status.success());
    assert!(
        String::from_utf8_lossy(&extra.stderr)
            .starts_with("ty-scip: expected at most two arguments\nUsage: ty-scip")
    );

    for directory in [zero_argument_project, caller, target] {
        fs::remove_dir_all(directory).expect("remove temporary project");
    }
}
