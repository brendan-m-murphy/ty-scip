use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use protobuf::Message;
use scip::types::Index;

#[test]
fn refuses_to_overwrite_input_scip_with_graph_json() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("scip-graphify-cli-{nonce}"));
    fs::create_dir_all(&directory).expect("directory");
    let input = directory.join("index.scip");
    let original = Index::new().write_to_bytes().expect("encode index");
    fs::write(&input, &original).expect("write index");

    let binary = option_env!("CARGO_BIN_EXE_scip-graphify")
        .or(option_env!("CARGO_BIN_EXE_scip_graphify"))
        .expect("Cargo must provide the converter binary path");
    let result = Command::new(binary)
        .args([input.as_os_str(), input.as_os_str()])
        .output()
        .expect("run converter");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("same file"));
    assert_eq!(fs::read(&input).expect("read index"), original);
    fs::remove_dir_all(directory).expect("cleanup");
}
