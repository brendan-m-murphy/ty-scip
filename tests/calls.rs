use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn emits_ty_ide_navigation_results() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ty-scip-calls-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).expect("create project");
    fs::write(
        root.join("main.py"),
        concat!(
            "def leaf():\n    return 1\n\n",
            "class Worker:\n    def run(self):\n        return 2\n\n",
            "def wrapper(worker: Worker):\n",
            "    leaf()\n",
            "    worker.run()\n",
            "    saved = worker.run\n",
            "    callback = leaf\n",
            "    callback()\n",
            "    return saved\n",
        ),
    )
    .expect("write source");
    let index = root.join("index.scip");
    let facts = root.join("index.tyfacts");

    let output = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&index)
        .args(["--facts", facts.to_str().expect("UTF-8 test path")])
        .output()
        .expect("run ty-scip");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let index_bytes = fs::read(&index).expect("read SCIP index");
    let sidecar: Value =
        serde_json::from_slice(&fs::read(&facts).expect("read ty facts")).expect("decode ty facts");
    assert_eq!(sidecar["format"], "ty-scip-facts");
    assert_eq!(sidecar["version"], 2);
    assert_eq!(
        sidecar["index_sha256"],
        format!("{:x}", Sha256::digest(index_bytes))
    );
    let symbols = sidecar["symbols"].as_array().expect("symbols array");
    let wrapper = symbols
        .iter()
        .find(|fact| {
            fact["symbol"]
                .as_str()
                .is_some_and(|symbol| symbol.ends_with("wrapper()."))
        })
        .expect("wrapper IDE results");
    assert_eq!(wrapper["hover"], "def wrapper(worker: Worker) -> Unknown");
    let calls = wrapper["outgoing_calls"]
        .as_array()
        .expect("outgoing calls");
    assert_eq!(calls.len(), 2, "{calls:#?}");
    let leaf = calls
        .iter()
        .find(|call| call["item"]["name"] == "leaf")
        .expect("leaf call");
    assert_eq!(
        leaf["from_ranges"],
        serde_json::json!([[8, 4, 8], [12, 4, 12]])
    );
    let run = calls
        .iter()
        .find(|call| call["item"]["name"] == "run")
        .expect("method call");
    assert_eq!(run["from_ranges"], serde_json::json!([[9, 11, 14]]));

    let first_sidecar = fs::read(&facts).expect("read first ty facts");
    let repeat = Command::new(env!("CARGO_BIN_EXE_ty-scip"))
        .arg(&root)
        .arg(&index)
        .args(["--facts", facts.to_str().expect("UTF-8 test path")])
        .output()
        .expect("run ty-scip again");
    assert!(
        repeat.status.success(),
        "{}",
        String::from_utf8_lossy(&repeat.stderr)
    );
    assert_eq!(
        fs::read(&facts).expect("read repeated ty facts"),
        first_sidecar,
        "static IDE results should be reproducible"
    );

    fs::remove_dir_all(root).expect("remove project");
}
