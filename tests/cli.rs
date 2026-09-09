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
    assert_eq!(
        String::from_utf8_lossy(&help.stdout),
        concat!(
            "Usage: ty-scip [index] [OPTIONS] [PROJECT_PATH] [OUTPUT.scip]\n",
            "\n",
            "Indexes a Python project into index.scip by default.\n",
            "\n",
            "Options:\n",
            "  --output PATH              Write the index to PATH\n",
            "  --cwd PATH                 Resolve relative paths from PATH\n",
            "  --quiet                    Suppress indexing diagnostics\n",
            "  --project-name NAME        Override the SCIP package name\n",
            "  --project-version VERSION  Override the SCIP package version\n",
            "  -h, --help                 Print help\n",
            "  -V, --version              Print version\n",
        )
    );
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

    let missing_project = caller.join("missing-project");
    let unresolved = Command::new(binary)
        .arg(&missing_project)
        .output()
        .expect("reject missing project path");
    assert!(!unresolved.status.success());
    assert!(
        String::from_utf8_lossy(&unresolved.stderr).starts_with(&format!(
            "ty-scip: cannot resolve project path {}:",
            missing_project.display()
        ))
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

#[test]
fn supports_scip_python_style_index_command() {
    let binary = env!("CARGO_BIN_EXE_ty-scip");
    let caller = project("compat-caller");
    let target = project("compat-target");

    let indexed = Command::new(binary)
        .current_dir(&caller)
        .args(["index", "--cwd"])
        .arg(&target)
        .args(["--output", "custom.scip", "--quiet"])
        .output()
        .expect("index with compatible command shape");
    assert!(
        indexed.status.success(),
        "{}",
        String::from_utf8_lossy(&indexed.stderr)
    );
    assert!(indexed.stdout.is_empty());
    assert!(indexed.stderr.is_empty());
    assert!(target.join("custom.scip").is_file());

    let conflict = Command::new(binary)
        .args([
            "index",
            "project",
            "positional.scip",
            "--output",
            "option.scip",
        ])
        .output()
        .expect("reject two output destinations");
    assert!(!conflict.status.success());
    assert_eq!(
        String::from_utf8_lossy(&conflict.stderr).trim(),
        "ty-scip: cannot use positional OUTPUT.scip with --output"
    );

    let unsupported = Command::new(binary)
        .args(["index", "--environment", "venv"])
        .output()
        .expect("reject unsupported scip-python option");
    assert!(!unsupported.status.success());
    assert_eq!(
        String::from_utf8_lossy(&unsupported.stderr).trim(),
        "ty-scip: unknown option --environment; try `ty-scip --help`"
    );

    for directory in [caller, target] {
        fs::remove_dir_all(directory).expect("remove temporary project");
    }
}
