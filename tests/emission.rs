use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use protobuf::Message;
use scip::types::Index;
use url::Url;

fn project() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ty-scip emission # π-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create project");
    fs::write(root.join("main.py"), "answer = 42\n").expect("write source");
    root.canonicalize().expect("canonical project root")
}

fn run(root: &Path, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(root)
        .arg(output)
        .output()
        .expect("run ty-scip")
}

fn read_index(path: &Path) -> Index {
    Index::parse_from_bytes(&fs::read(path).expect("read SCIP index")).expect("decode SCIP index")
}

fn assert_no_temporary_files(root: &Path) {
    let temporary = fs::read_dir(root)
        .expect("read project directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".ty-scip-") && name.ends_with(".tmp"))
        .collect::<Vec<_>>();
    assert!(
        temporary.is_empty(),
        "leftover temporary files: {temporary:?}"
    );
}

#[test]
fn output_is_reproducible_and_atomically_replaced() {
    let root = project();
    let first = root.join("first.scip");
    let second = root.join("second.scip");

    for output in [&first, &second] {
        let result = run(&root, output);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let first_bytes = fs::read(&first).expect("read first index");
    assert_eq!(first_bytes, fs::read(&second).expect("read second index"));

    let index = read_index(&first);
    let metadata = index.metadata.as_ref().expect("index metadata");
    assert_eq!(
        metadata.project_root,
        Url::from_file_path(&root)
            .expect("project file URI")
            .as_str()
    );
    assert!(metadata.project_root.contains("%20%23%20%CF%80"));
    assert!(
        metadata
            .tool_info
            .as_ref()
            .expect("tool information")
            .arguments
            .is_empty()
    );

    fs::write(&first, b"not an index").expect("replace index with junk");
    let replaced = run(&root, &first);
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    read_index(&first);
    assert_no_temporary_files(&root);

    let blocked = root.join("blocked.scip");
    fs::create_dir(&blocked).expect("create blocking output directory");
    let failed = run(&root, &blocked);
    assert!(!failed.status.success());
    assert_no_temporary_files(&root);

    fs::remove_dir_all(root).expect("remove project");
}

#[test]
fn invalid_utf8_source_fails_without_writing_an_index() {
    let root = project();
    fs::write(root.join("main.py"), b"value = \xff\n").expect("write invalid source");
    let output = root.join("index.scip");

    let result = run(&root, &output);

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("cannot read project file"), "{stderr}");
    assert!(stderr.contains("main.py"), "{stderr}");
    assert!(!output.exists());

    fs::write(&output, b"existing index").expect("write existing output");
    let repeated = run(&root, &output);
    assert!(!repeated.status.success());
    assert_eq!(
        fs::read(&output).expect("read preserved output"),
        b"existing index"
    );
    fs::remove_dir_all(root).expect("remove project");
}
