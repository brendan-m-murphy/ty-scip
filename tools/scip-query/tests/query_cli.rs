use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use protobuf::Message;
use scip::types::{
    Document, Index, Occurrence, Relationship, Signature, SymbolInformation, SymbolRole,
    symbol_information,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BASE: &str = "example package demo 1.0 pkg/Base#";
const ALPHA: &str = "example package demo 1.0 pkg/Alpha#";
const RUN: &str = "example package demo 1.0 pkg/Alpha#run().";
const HELPER: &str = "example package demo 1.0 pkg/helper().";
const TEST: &str = "example package demo 1.0 tests/test_run().";
static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    index: PathBuf,
    facts: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "scip-query-offline-lsp-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let index = root.join("index.scip");
        let facts = root.join("index.tyfacts");
        let bytes = synthetic_index().write_to_bytes().unwrap();
        fs::write(&index, &bytes).unwrap();
        fs::write(
            &facts,
            serde_json::to_vec_pretty(&json!({
                "format": "ty-scip-facts",
                "version": 2,
                "index_sha256": format!("{:x}", Sha256::digest(&bytes)),
                "symbols": [
                    {
                        "document": "pkg/models.py",
                        "symbol": RUN,
                        "item": {
                            "document": "pkg/models.py",
                            "symbol": RUN,
                            "name": "run",
                            "detail": "pkg.models",
                            "range": [1, 8, 11],
                            "full_range": [1, 4, 5, 0]
                        },
                        "definitions": [{
                            "document": "pkg/models.py",
                            "range": [1, 8, 11],
                            "full_range": [1, 4, 5, 0]
                        }],
                        "hover": "def run(self) -> int",
                        "references": [{
                            "document": "tests/test_models.py",
                            "range": [2, 4, 7],
                            "reference_kind": "read"
                        }],
                        "incoming_calls": [{
                            "item": {
                                "document": "tests/test_models.py",
                                "symbol": TEST,
                                "name": "test_run",
                                "detail": "tests.test_models",
                                "range": [0, 4, 12],
                                "full_range": [0, 0, 3, 0]
                            },
                            "from_ranges": [[2, 4, 7]]
                        }],
                        "outgoing_calls": [{
                            "item": {
                                "document": "pkg/helper.py",
                                "symbol": HELPER,
                                "name": "helper",
                                "detail": "pkg.helper",
                                "range": [0, 4, 10],
                                "full_range": [0, 0, 2, 0]
                            },
                            "from_ranges": [[3, 8, 14], [4, 8, 14]]
                        }],
                        "supertypes": [],
                        "subtypes": []
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        Self { root, index, facts }
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_scip-query"))
            .args(["--index", self.index.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> Value {
        let output = self.command(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn info(symbol: &str, display: &str, kind: symbol_information::Kind) -> SymbolInformation {
    SymbolInformation {
        symbol: symbol.into(),
        display_name: display.into(),
        kind: kind.into(),
        ..Default::default()
    }
}

fn definition(symbol: &str, range: &[i32], enclosing: &[i32]) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range: range.into(),
        enclosing_range: enclosing.into(),
        symbol_roles: SymbolRole::Definition as i32,
        ..Default::default()
    }
}

fn reference(symbol: &str, range: &[i32]) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range: range.into(),
        symbol_roles: SymbolRole::ReadAccess as i32,
        ..Default::default()
    }
}

fn synthetic_index() -> Index {
    let mut alpha = info(ALPHA, "Alpha", symbol_information::Kind::Class);
    alpha.relationships.push(Relationship {
        symbol: BASE.into(),
        is_implementation: true,
        ..Default::default()
    });
    let mut run = info(RUN, "run", symbol_information::Kind::Method);
    run.enclosing_symbol = ALPHA.into();
    run.documentation = vec!["Run the model.".into()];
    run.signature_documentation = Some(Signature {
        language: "python".into(),
        text: "def run(self)".into(),
        ..Default::default()
    })
    .into();
    Index {
        documents: vec![
            Document {
                relative_path: "pkg/models.py".into(),
                text: "class Alpha:\n    def run(self):\n        value = 1\n        helper()\n        helper()\n"
                    .into(),
                symbols: vec![
                    alpha,
                    run,
                    info(BASE, "Base", symbol_information::Kind::Class),
                ],
                occurrences: vec![
                    definition(ALPHA, &[0, 6, 11], &[0, 0, 5, 0]),
                    definition(RUN, &[1, 8, 11], &[1, 4, 5, 0]),
                    reference(HELPER, &[3, 8, 14]),
                    reference(HELPER, &[4, 8, 14]),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "pkg/helper.py".into(),
                text: "def helper():\n    return 1\n".into(),
                symbols: vec![info(HELPER, "helper", symbol_information::Kind::Function)],
                occurrences: vec![definition(HELPER, &[0, 4, 10], &[0, 0, 2, 0])],
                ..Default::default()
            },
            Document {
                relative_path: "tests/test_models.py".into(),
                text: "def test_run(model):\n    model = model\n    model.run()\n".into(),
                symbols: vec![info(TEST, "test_run", symbol_information::Kind::Function)],
                occurrences: vec![
                    definition(TEST, &[0, 4, 12], &[0, 0, 3, 0]),
                    reference(RUN, &[2, 4, 7]),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "tests/test_import.py".into(),
                text: "from pkg.models import Alpha\n".into(),
                symbols: vec![info("local 0", "Alpha", symbol_information::Kind::Module)],
                occurrences: vec![
                    Occurrence {
                        symbol: "local 0".into(),
                        range: vec![0, 23, 28],
                        symbol_roles: SymbolRole::Definition as i32 | SymbolRole::Import as i32,
                        ..Default::default()
                    },
                    Occurrence {
                        symbol: ALPHA.into(),
                        range: vec![0, 23, 28],
                        symbol_roles: SymbolRole::Import as i32,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn exposes_the_minimal_navigation_surface() {
    let fixture = Fixture::new();

    assert_eq!(fixture.success(&["find", "Alpha"])["result"]["total"], 1);
    assert_eq!(
        fixture.success(&["at", "pkg/models.py:2:9"])["result"]["items"][0]["symbol"]["selector"],
        "pkg.Alpha.run",
    );
    assert_eq!(
        fixture.success(&["hover", "Alpha.run"])["result"]["contents"],
        "def run(self) -> int"
    );
    assert_eq!(
        fixture.success(&["definition", "Alpha.run"])["result"]["total"],
        1
    );
    assert_eq!(
        fixture.success(&["references", "Alpha.run", "--path", "tests/"])["result"]["total"],
        1
    );
    assert_eq!(
        fixture.success(&["members", "Alpha"])["result"]["items"][0]["name"],
        "run"
    );
    assert_eq!(
        fixture.success(&["supertypes", "Alpha"])["result"]["items"][0]["selector"],
        "pkg.Base"
    );
    assert_eq!(
        fixture.success(&["subtypes", "Base"])["result"]["items"][0]["selector"],
        "pkg.Alpha"
    );

    assert_eq!(
        fixture.success(&["definition", "Alpha"])["resolved"],
        "pkg.Alpha"
    );
    let import = fixture.success(&["at", "tests/test_import.py:1:24"]);
    assert_eq!(import["result"]["total"], 1);
    assert_eq!(
        import["result"]["items"][0]["symbol"]["selector"],
        "pkg.Alpha"
    );
}

#[test]
fn uses_fingerprinted_ty_ide_results_for_navigation() {
    let fixture = Fixture::new();

    let callees = fixture.success(&["callees", "Alpha.run"]);
    assert_eq!(callees["result"]["total"], 1);
    assert_eq!(
        callees["result"]["items"][0]["to"]["selector"],
        "pkg.helper"
    );

    let hover = fixture.success(&["hover", "Alpha.run"]);
    assert_eq!(hover["result"]["contents"], "def run(self) -> int");
    let references = fixture.success(&["references", "Alpha.run"]);
    assert_eq!(references["result"]["items"][0]["reference_kind"], "read");
    assert_eq!(
        callees["result"]["items"][0]["from_ranges"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let callers = fixture.success(&["callers", "Alpha.run"]);
    assert_eq!(callers["result"]["total"], 1);
    assert_eq!(
        callers["result"]["items"][0]["from"]["selector"],
        "tests.test_run"
    );

    fs::write(
        &fixture.facts,
        json!({
            "format": "ty-scip-facts",
            "version": 2,
            "index_sha256": "stale",
            "symbols": [],
        })
        .to_string(),
    )
    .unwrap();
    let stale = fixture.command(&["callers", "Alpha.run"]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("fingerprint does not match"));
}

#[test]
fn requires_ty_facts_only_for_the_call_hierarchy() {
    let fixture = Fixture::new();
    fs::remove_file(&fixture.facts).unwrap();

    assert!(fixture.command(&["find", "Alpha"]).status.success());
    let callers = fixture.command(&["callers", "Alpha.run"]);
    assert!(!callers.status.success());
    assert!(String::from_utf8_lossy(&callers.stderr).contains("requires a synchronized ty facts"));
}
