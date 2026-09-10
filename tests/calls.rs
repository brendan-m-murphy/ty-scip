use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn emits_only_resolved_references_in_callee_position() {
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
    assert_eq!(sidecar["version"], 1);
    assert_eq!(
        sidecar["index_sha256"],
        format!("{:x}", Sha256::digest(index_bytes))
    );
    let facts = sidecar["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(
        facts
            .iter()
            .all(|fact| fact["kind"] == "callee_position" && fact["document"] == "main.py")
    );
    assert_eq!(facts[0]["range"], serde_json::json!([8, 4, 8]));
    assert_eq!(facts[1]["range"], serde_json::json!([9, 11, 14]));
    assert!(
        facts[0]["enclosing_symbol"]
            .as_str()
            .is_some_and(|symbol| symbol.ends_with("wrapper()."))
    );
    assert!(
        facts[0]["symbol"]
            .as_str()
            .is_some_and(|symbol| symbol.ends_with("leaf()."))
    );
    assert!(
        facts[1]["symbol"]
            .as_str()
            .is_some_and(|symbol| symbol.ends_with("Worker#run()."))
    );

    fs::remove_dir_all(root).expect("remove project");
}
