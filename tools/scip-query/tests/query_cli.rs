use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use protobuf::Message;
use scip::types::{
    Document, Index, MultiLineRange, Occurrence, Relationship, SymbolInformation, SymbolRole,
    symbol_information,
};
use serde_json::Value;

const ALPHA: &str = "example package demo 1.0 pkg/Alpha#";
const ALPHA_RUN: &str = "example package demo 1.0 pkg/Alpha#run().";
const BETA: &str = "example package demo 1.0 pkg/Beta#";
const BETA_RUN: &str = "example package demo 1.0 pkg/Beta#run().";
const BASE: &str = "example package demo 1.0 pkg/Base#";
const HELPER: &str = "example package demo 1.0 pkg/helper().";
const LEAF: &str = "example package demo 1.0 pkg/leaf().";

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    index: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "scip-query-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("pkg")).unwrap();
        fs::write(
            root.join("pkg/models.py"),
            "class Alpha:\n    def run(self):\n        helper()\n        leaf()\n\nclass Beta:\n    def run(self):\n        helper()\n",
        )
        .unwrap();
        fs::write(
            root.join("pkg/models.pyi"),
            "class Alpha:\n    def run(self) -> None: ...\n",
        )
        .unwrap();
        fs::write(root.join("pkg/util.py"), "def helper():\n    leaf()\n").unwrap();
        fs::write(root.join("pkg/leaf.py"), "def leaf():\n    return 1\n").unwrap();
        fs::write(root.join("pkg/other.py"), "value = 1\nprint(value)\n").unwrap();

        let index = root.join("index.scip");
        fs::write(&index, synthetic_index().write_to_bytes().unwrap()).unwrap();
        Self { root, index }
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scip-query"))
            .arg("--index")
            .arg(&self.index)
            .arg("--root")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> Value {
        self.json(args, 0)
    }

    fn json(&self, args: &[&str], expected_code: i32) -> Value {
        let output = self.command(args);
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "unexpected status for {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "command {args:?} did not emit JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn info(symbol: &str, display_name: &str, kind: symbol_information::Kind) -> SymbolInformation {
    SymbolInformation {
        symbol: symbol.into(),
        display_name: display_name.into(),
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

fn reference(symbol: &str, range: &[i32], roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range: range.into(),
        symbol_roles: roles,
        ..Default::default()
    }
}

fn typed_reference(symbol: &str, roles: i32, range: [i32; 4]) -> Occurrence {
    let mut occurrence = reference(symbol, &[88, 88, 99], roles);
    occurrence.set_multi_line_range(MultiLineRange {
        start_line: range[0],
        start_character: range[1],
        end_line: range[2],
        end_character: range[3],
        ..Default::default()
    });
    occurrence
}

fn synthetic_index() -> Index {
    let mut alpha = info(ALPHA, "Alpha", symbol_information::Kind::Class);
    alpha.relationships.push(Relationship {
        symbol: BASE.into(),
        is_reference: true,
        is_implementation: true,
        is_type_definition: true,
        is_definition: true,
        ..Default::default()
    });
    let mut alpha_run = info(ALPHA_RUN, "run", symbol_information::Kind::Method);
    alpha_run.enclosing_symbol = ALPHA.into();
    let mut beta_run = info(BETA_RUN, "run", symbol_information::Kind::Method);
    beta_run.enclosing_symbol = BETA.into();

    let duplicate = reference(HELPER, &[2, 8, 14], SymbolRole::ReadAccess as i32);
    Index {
        documents: vec![
            Document {
                relative_path: "pkg/models.py".into(),
                symbols: vec![
                    alpha,
                    alpha_run,
                    info(BETA, "Beta", symbol_information::Kind::Class),
                    beta_run,
                ],
                occurrences: vec![
                    definition(ALPHA, &[0, 6, 11], &[0, 0, 4, 0]),
                    definition(ALPHA_RUN, &[1, 8, 11], &[1, 4, 4, 0]),
                    duplicate.clone(),
                    duplicate,
                    reference(LEAF, &[3, 8, 12], SymbolRole::WriteAccess as i32),
                    definition(BETA, &[5, 6, 10], &[5, 0, 8, 0]),
                    definition(BETA_RUN, &[6, 8, 11], &[6, 4, 8, 0]),
                    reference(HELPER, &[7, 8, 14], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "pkg/models.pyi".into(),
                symbols: vec![info(ALPHA, "Alpha", symbol_information::Kind::Class)],
                occurrences: vec![definition(ALPHA, &[0, 6, 11], &[0, 0, 2, 0])],
                ..Default::default()
            },
            Document {
                relative_path: "pkg/util.py".into(),
                symbols: vec![info(HELPER, "helper", symbol_information::Kind::Function)],
                occurrences: vec![
                    definition(HELPER, &[0, 4, 10], &[0, 0, 2, 0]),
                    typed_reference(LEAF, SymbolRole::ReadAccess as i32, [1, 4, 1, 8]),
                    definition("local 0", &[0, 11, 12], &[0, 0, 2, 0]),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "pkg/leaf.py".into(),
                symbols: vec![info(LEAF, "leaf", symbol_information::Kind::Function)],
                occurrences: vec![definition(LEAF, &[0, 4, 8], &[0, 0, 2, 0])],
                ..Default::default()
            },
            Document {
                relative_path: "pkg/other.py".into(),
                symbols: vec![info("local 0", "value", symbol_information::Kind::Variable)],
                occurrences: vec![
                    definition("local 0", &[0, 0, 5], &[0, 0, 2, 0]),
                    reference("local 0", &[1, 6, 11], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
        ],
        external_symbols: vec![info(BASE, "Base", symbol_information::Kind::Class)],
        ..Default::default()
    }
}

fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn assert_contains(value: &Value, needle: &str) {
    let text = json_text(value);
    assert!(text.contains(needle), "expected {needle:?} in {text}");
}

fn result_items(value: &Value) -> &[Value] {
    value
        .pointer("/result/items")
        .and_then(Value::as_array)
        .expect("result.items")
}

#[test]
fn qualified_member_resolution_is_exact_and_bare_member_is_ambiguous() {
    let fixture = Fixture::new();

    let scoped_find = fixture.success(&["find", "models", "--path", "pkg/models.py"]);
    assert!(result_items(&scoped_find).iter().all(|item| {
        item["id"].get("document").is_none() || item["id"]["symbol"] == "<document>"
    }));

    let exact = fixture.success(&["context", "pkg/models.py:Alpha.run"]);
    assert_contains(&exact, ALPHA_RUN);
    assert_contains(&exact, "pkg/models.py");

    let ambiguous = fixture.json(&["context", "run"], 2);
    assert_eq!(ambiguous["status"], "ambiguous");
    let candidates = ambiguous["candidates"]["items"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .any(|item| item["id"]["symbol"] == ALPHA_RUN)
    );
    assert!(
        candidates
            .iter()
            .any(|item| item["id"]["symbol"] == BETA_RUN)
    );
}

#[test]
fn occurrences_keep_multiplicity_roles_typed_ranges_and_document_local_identity() {
    let fixture = Fixture::new();

    let refs = fixture.success(&["refs", "pkg/models.py:Alpha.run", "--outgoing"]);
    let items = result_items(&refs);
    let helper_refs: Vec<_> = items
        .iter()
        .filter(|item| item["target"]["symbol"] == HELPER)
        .collect();
    assert_eq!(helper_refs.len(), 2, "duplicate occurrences lost: {refs}");
    assert!(helper_refs.iter().all(|item| {
        item["evidence"]["occurrence"]["symbol_roles"] == SymbolRole::ReadAccess as i32
            && item["evidence"]["occurrence"]["role_names"] == serde_json::json!(["read"])
            && item["evidence"]["occurrence"]["range"]["start"]["line"] == 2
            && item["evidence"]["occurrence"]["range"]["start"]["character"] == 8
            && item["evidence"]["occurrence"]["range"]["end"]["character"] == 14
    }));
    assert!(items.iter().any(|item| {
        item["target"]["symbol"] == LEAF
            && item["evidence"]["occurrence"]["symbol_roles"] == SymbolRole::WriteAccess as i32
    }));

    let at = fixture.success(&["at", "pkg/util.py:2:5"]);
    let occurrence = &result_items(&at)[0];
    assert_eq!(occurrence["symbol"]["symbol"], LEAF);
    assert_eq!(occurrence["range_source"], "typed");
    assert_eq!(occurrence["range"]["start"]["line"], 1);
    assert_eq!(occurrence["range"]["start"]["character"], 4);
    assert_eq!(occurrence["range"]["end"]["character"], 8);
    assert_eq!(occurrence["legacy_range"], serde_json::json!([88, 88, 99]));

    let local = fixture.success(&["at", "pkg/other.py:2:8"]);
    let occurrence = &result_items(&local)[0];
    assert_eq!(occurrence["symbol"]["symbol"], "local 0");
    assert_eq!(occurrence["symbol"]["document"], "pkg/other.py");
}

#[test]
fn relationships_ownership_members_and_traversal_keep_occurrence_evidence() {
    let fixture = Fixture::new();

    let alpha = fixture.success(&["context", "pkg/models.py:Alpha"]);
    for flag in [
        "is_reference",
        "is_implementation",
        "is_type_definition",
        "is_definition",
    ] {
        assert_eq!(alpha["result"]["relationships"]["items"][0][flag], true);
    }
    assert_eq!(alpha["result"]["definitions"]["total"], 2);
    let definitions = alpha["result"]["definitions"]["items"].as_array().unwrap();
    assert!(
        definitions
            .iter()
            .any(|item| item["document"] == "pkg/models.py")
    );
    assert!(
        definitions
            .iter()
            .any(|item| item["document"] == "pkg/models.pyi")
    );

    let members = fixture.success(&["members", "pkg/models.py:Alpha"]);
    let member_items = result_items(&members);
    assert!(
        member_items
            .iter()
            .any(|item| item["symbol"]["id"]["symbol"] == ALPHA_RUN)
    );
    assert!(
        !member_items
            .iter()
            .any(|item| item["symbol"]["id"]["symbol"] == BETA_RUN)
    );

    let outgoing = fixture.success(&["refs", "pkg/models.py:Alpha.run", "--outgoing"]);
    assert!(result_items(&outgoing).iter().all(|item| {
        item["source"]["symbol"] == ALPHA_RUN
            && item["evidence"]["occurrence"]["owner"]["symbol"] == ALPHA_RUN
            && item["evidence"]["occurrence"]["document"] == "pkg/models.py"
    }));

    let path = fixture.success(&["path", ALPHA_RUN, LEAF, "--max-depth", "1"]);
    assert_eq!(path["result"]["found"], true);
    assert_eq!(path["result"]["steps"]["total"], 1);
    let edge = &path["result"]["steps"]["items"][0];
    assert_eq!(edge["source"]["symbol"], ALPHA_RUN);
    assert_eq!(edge["target"]["symbol"], LEAF);
    assert_eq!(edge["evidence"]["occurrence"]["range"]["start"]["line"], 3);

    let whole_path = fixture.success(&["--limit", "1", "path", BETA_RUN, LEAF, "--max-depth", "2"]);
    assert_eq!(whole_path["result"]["steps"]["total"], 2);
    assert_eq!(whole_path["result"]["steps"]["returned"], 2);
    assert_eq!(whole_path["result"]["steps"]["truncated"], false);

    let affected = fixture.success(&["affected", LEAF, "--max-depth", "2"]);
    let affected = result_items(&affected);
    let helper = affected
        .iter()
        .find(|item| item["symbol"]["id"]["symbol"] == HELPER)
        .unwrap();
    assert_eq!(helper["predecessor"]["source"]["symbol"], HELPER);
    assert_eq!(helper["predecessor"]["target"]["symbol"], LEAF);
    assert!(
        affected
            .iter()
            .any(|item| item["symbol"]["id"]["symbol"] == HELPER)
    );
    assert!(
        affected
            .iter()
            .any(|item| item["symbol"]["id"]["symbol"] == ALPHA_RUN)
    );
}

#[test]
fn truncation_and_source_snippets_are_deterministic() {
    let fixture = Fixture::new();
    let first = fixture.success(&["--limit", "1", "find", "run"]);
    let second = fixture.success(&["--limit", "1", "find", "run"]);
    assert_eq!(first, second);
    assert_eq!(first["result"]["truncated"], true);
    assert_eq!(first["result"]["total"], 2);
    assert_eq!(first["result"]["returned"], 1);

    let context = fixture.success(&["context", "pkg/models.py:Alpha.run"]);
    let snippets = context["result"]["snippets"]["items"].as_array().unwrap();
    assert!(snippets.iter().any(|snippet| {
        snippet["text"]
            .as_str()
            .is_some_and(|text| text.contains("def run(self):") && text.contains("helper()"))
    }));
}
