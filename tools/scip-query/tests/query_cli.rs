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
const TEST_ALPHA: &str = "example package demo 1.0 tests/TestAlpha#";
const TEST_LEAF: &str = "example package demo 1.0 tests/test_leaf().";
const RECURSE: &str = "example package demo 1.0 pkg/recurse().";

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
        fs::create_dir_all(root.join("tests")).unwrap();
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
        fs::write(
            root.join("tests/test_models.py"),
            "from pkg import Alpha\n\ndef test_alpha():\n    value = Alpha()\n    value.run()\n\nclass TestAlpha(Alpha):\n    pass\n",
        )
        .unwrap();
        fs::write(
            root.join("tests/test_leaf.py"),
            "from pkg.leaf import leaf\n\ndef test_leaf():\n    assert leaf() == 1\n",
        )
        .unwrap();

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
    let mut test_alpha = info(TEST_ALPHA, "TestAlpha", symbol_information::Kind::Class);
    test_alpha.relationships.push(Relationship {
        symbol: ALPHA.into(),
        is_implementation: true,
        is_type_definition: true,
        ..Default::default()
    });

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
            Document {
                relative_path: "tests/test_models.py".into(),
                symbols: vec![
                    info("local 0", "Alpha", symbol_information::Kind::Variable),
                    info("local 1", "Model", symbol_information::Kind::Variable),
                    test_alpha,
                ],
                occurrences: vec![
                    reference(ALPHA, &[0, 16, 21], SymbolRole::Import as i32),
                    reference(
                        "local 0",
                        &[0, 16, 21],
                        SymbolRole::Definition as i32 | SymbolRole::Import as i32,
                    ),
                    reference(ALPHA, &[0, 16, 21], SymbolRole::Import as i32),
                    reference(
                        "local 1",
                        &[0, 16, 21],
                        SymbolRole::Definition as i32 | SymbolRole::Import as i32,
                    ),
                    reference(ALPHA, &[3, 12, 17], SymbolRole::ReadAccess as i32),
                    reference(ALPHA_RUN, &[4, 10, 13], SymbolRole::ReadAccess as i32),
                    definition(TEST_ALPHA, &[6, 6, 15], &[6, 0, 8, 0]),
                    reference(ALPHA, &[6, 16, 21], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "tests/test_leaf.py".into(),
                text: "from pkg.leaf import leaf\n\ndef test_leaf():\n    assert leaf() == 1\n"
                    .into(),
                symbols: vec![info(
                    TEST_LEAF,
                    "test_leaf",
                    symbol_information::Kind::Function,
                )],
                occurrences: vec![
                    definition(TEST_LEAF, &[2, 4, 13], &[2, 0, 4, 0]),
                    reference(LEAF, &[3, 11, 15], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
        ],
        external_symbols: vec![info(BASE, "Base", symbol_information::Kind::Class)],
        ..Default::default()
    }
}

fn recursive_index() -> Index {
    let mut recurse = info(RECURSE, "recurse", symbol_information::Kind::Function);
    recurse.relationships.push(Relationship {
        symbol: RECURSE.into(),
        is_reference: true,
        ..Default::default()
    });
    Index {
        documents: vec![Document {
            relative_path: "pkg/recurse.py".into(),
            symbols: vec![recurse],
            occurrences: vec![
                definition(RECURSE, &[0, 4, 11], &[0, 0, 12, 0]),
                reference(RECURSE, &[1, 20, 27], SymbolRole::ReadAccess as i32),
                reference(RECURSE, &[9, 1, 8], SymbolRole::ReadAccess as i32),
                reference(RECURSE, &[1, 5, 12], SymbolRole::ReadAccess as i32),
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn path_prefix_index() -> Index {
    let documents = [
        "tests_/literal.py",
        "testsa/wildcard.py",
        "tests%/literal.py",
        "testsx/wildcard.py",
        "TESTS_/case.py",
    ]
    .into_iter()
    .map(|path| Document {
        relative_path: path.into(),
        occurrences: vec![reference(ALPHA, &[0, 0, 5], SymbolRole::ReadAccess as i32)],
        ..Default::default()
    })
    .collect();
    Index {
        documents,
        external_symbols: vec![info(ALPHA, "Alpha", symbol_information::Kind::Class)],
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

    let hash_alias = fixture.success(&["refs", "pkg.Alpha#run", "--outgoing", "--compact"]);
    assert_eq!(hash_alias["resolved"]["symbol"], ALPHA_RUN);

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
fn compact_refs_support_path_filtering_and_pagination() {
    let fixture = Fixture::new();

    let first = fixture.success(&[
        "refs",
        "pkg/models.py:Alpha.run",
        "--outgoing",
        "--path",
        "pkg/models.py",
        "--compact",
        "--limit",
        "1",
    ]);
    assert_eq!(first["compact"], true);
    assert_eq!(first["result"]["total"], 3);
    assert_eq!(first["result"]["returned"], 1);
    assert_eq!(first["result"]["next_offset"], 1);
    let item = &result_items(&first)[0];
    assert_eq!(item["source"], "pkg.Alpha.run");
    assert_eq!(item["source_selector"], ALPHA_RUN);
    assert!(
        item["target_selector"]
            .as_str()
            .unwrap()
            .contains("example package")
    );
    assert_eq!(item["evidence"]["document"], "pkg/models.py");
    assert!(item["evidence"].get("legacy_range").is_none());

    let second = fixture.success(&[
        "refs",
        "pkg/models.py:Alpha.run",
        "--outgoing",
        "--path",
        "pkg/models.py",
        "--compact",
        "--offset",
        "1",
        "--limit",
        "2",
    ]);
    assert_eq!(second["result"]["offset"], 1);
    assert_eq!(second["result"]["returned"], 2);
    assert!(second["result"]["next_offset"].is_null());
}

#[test]
fn recursive_references_are_deduplicated_without_losing_occurrences() {
    let fixture = Fixture::new();
    fs::write(&fixture.index, recursive_index().write_to_bytes().unwrap()).unwrap();

    let direct = fixture.success(&["refs", RECURSE, "--both", "--limit", "10"]);
    assert_eq!(direct["result"]["total"], 4);

    let database = fixture.root.join("recursive.sqlite");
    let database_text = database.to_str().unwrap();
    fixture.success(&["build-db", database_text]);
    let sql = fixture.success(&[
        "sql-refs",
        database_text,
        RECURSE,
        "--both",
        "--limit",
        "10",
    ]);
    assert_eq!(sql["result"]["total"], 2);
    let occurrence = result_items(&sql)
        .iter()
        .find(|item| item["relationship"].is_null())
        .unwrap();
    assert_eq!(occurrence["occurrences"], 3);
    assert_eq!(occurrence["line"], 2);
    assert_eq!(occurrence["column"], 6);
}

#[test]
fn test_candidate_path_prefixes_are_literal_and_case_sensitive() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.index,
        path_prefix_index().write_to_bytes().unwrap(),
    )
    .unwrap();
    let database = fixture.root.join("prefix.sqlite");
    let database_text = database.to_str().unwrap();
    fixture.success(&["build-db", database_text]);

    for (prefix, expected) in [
        ("tests_/", "tests_/literal.py"),
        ("tests%/", "tests%/literal.py"),
        ("TESTS_/", "TESTS_/case.py"),
    ] {
        let candidates = fixture.success(&[
            "test-candidates",
            database_text,
            ALPHA,
            "--path",
            prefix,
            "--limit",
            "10",
        ]);
        let items = result_items(&candidates);
        assert_eq!(
            items.len(),
            1,
            "unexpected candidates for {prefix}: {candidates}"
        );
        assert_eq!(items[0]["document"], expected);
    }
}

#[test]
fn missing_selector_returns_bounded_suggestions() {
    let fixture = Fixture::new();
    let missing = fixture.json(&["refs", "pkg.Missing#run", "--limit", "1"], 2);
    assert_eq!(missing["status"], "not_found");
    assert_eq!(missing["suggestions"]["returned"], 1);
    assert_eq!(missing["suggestions"]["truncated"], true);
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

#[test]
fn sqlite_cache_preserves_facts_and_groups_reference_occurrences() {
    let fixture = Fixture::new();
    let database = fixture.root.join("lossless.sqlite");
    let database_text = database.to_str().unwrap();

    let built = fixture.success(&["build-db", database_text]);
    assert_eq!(built["result"]["documents"], 7);
    assert_eq!(built["result"]["occurrences"], 25);
    assert_eq!(built["result"]["relationships"], 2);

    let stats = fixture.success(&["sql-stats", database_text]);
    assert_eq!(stats["result"], built["result"]);

    let refs = fixture.success(&[
        "sql-refs",
        database_text,
        "pkg.Alpha#run",
        "--outgoing",
        "--path",
        "pkg/",
        "--limit",
        "10",
    ]);
    let items = result_items(&refs);
    let helper = items
        .iter()
        .find(|item| item["target"] == "pkg.helper")
        .unwrap();
    assert_eq!(helper["occurrences"], 2);
    assert_eq!(helper["line"], 3);
    assert_eq!(helper["low_signal"], false);
    assert!(items.iter().any(|item| item["target"] == "pkg.leaf"));

    let alpha = fixture.success(&["sql-refs", database_text, "pkg.Alpha", "--outgoing"]);
    assert!(result_items(&alpha).iter().any(|item| {
        item["target"] == "pkg.Base"
            && item["relationship"]["is_implementation"] == true
            && item["provenance"] == "scip_relationship"
    }));
}

#[test]
fn sqlite_resolves_import_aliases_and_projects_tests() {
    let fixture = Fixture::new();
    let database = fixture.root.join("lossless.sqlite");
    let database_text = database.to_str().unwrap();
    fixture.success(&["build-db", database_text]);

    let refs = fixture.success(&[
        "sql-refs",
        database_text,
        "Alpha",
        "--incoming",
        "--path",
        "tests/",
    ]);
    assert_eq!(refs["resolved"], "pkg.Alpha");
    assert!(
        result_items(&refs)
            .iter()
            .any(|item| item["document"] == "tests/test_models.py")
    );

    let alias = fixture.success(&["sql-refs", database_text, "Model", "--incoming"]);
    assert_eq!(alias["resolved"], "pkg.Alpha");

    let ambiguous = fixture.json(&["sql-refs", database_text, "run"], 2);
    assert_eq!(ambiguous["status"], "ambiguous");
    assert_eq!(ambiguous["candidates"].as_array().unwrap().len(), 2);

    let tests = fixture.success(&["test-candidates", database_text, "Alpha", "--limit", "10"]);
    assert_eq!(tests["resolved"], "pkg.Alpha");
    let items = result_items(&tests);
    assert!(
        items
            .iter()
            .all(|item| item["document"] == "tests/test_models.py")
    );
    assert!(items.iter().any(|item| item["match_kind"] == "symbol"));
    assert!(items.iter().any(|item| {
        item["match_kind"] == "symbol" && item["evidence_kind"] == "direct_reference"
    }));
    assert!(
        items
            .iter()
            .any(|item| { item["match_kind"] == "member" && item["target"] == "pkg.Alpha.run" })
    );
    assert!(
        items
            .iter()
            .any(|item| { item["match_kind"] == "subtype" && item["target"] == "tests.TestAlpha" })
    );

    let method_tests = fixture.success(&[
        "test-candidates",
        database_text,
        "pkg.Alpha#run",
        "--depth",
        "4",
        "--limit",
        "10",
    ]);
    assert!(result_items(&method_tests).iter().any(|item| {
        item["match_kind"] == "owner"
            && item["target"] == "pkg.Alpha"
            && item["document"] == "tests/test_models.py"
    }));
    let downstream = result_items(&method_tests)
        .iter()
        .find(|item| item["document"] == "tests/test_leaf.py")
        .expect("transitive callable references should project downstream tests");
    assert_eq!(downstream["depth"], 2);
    assert_eq!(
        downstream["path"],
        serde_json::json!(["pkg.Alpha.run", "pkg.helper", "pkg.leaf"])
    );

    let shallow = fixture.success(&[
        "test-candidates",
        database_text,
        "pkg.Alpha#run",
        "--depth",
        "1",
        "--limit",
        "10",
    ]);
    assert!(
        result_items(&shallow)
            .iter()
            .all(|item| item["document"] != "tests/test_leaf.py")
    );

    let grouped = fixture.success(&[
        "test-candidates",
        database_text,
        "pkg.Alpha#run",
        "--group-files",
        "--depth",
        "4",
        "--limit",
        "10",
    ]);
    assert_eq!(grouped["group_by"], "file");
    assert_eq!(grouped["result"]["total"], 2);
    let leaf = result_items(&grouped)
        .iter()
        .find(|item| item["document"] == "tests/test_leaf.py")
        .unwrap();
    assert_eq!(leaf["depth"], 2);
    assert_eq!(
        leaf["representative_evidence_kind"],
        "transitive_callable_reference"
    );
    assert_eq!(
        leaf["evidence_kinds"],
        serde_json::json!(["transitive_callable_reference"])
    );
    assert_eq!(leaf["test_symbol"], "tests.test_leaf");
    assert_eq!(leaf["snippet"], "assert leaf() == 1");
    assert_eq!(leaf["terminal_symbol"], "pkg.leaf");
    assert_eq!(leaf["follow_up_selectors"], serde_json::json!(["pkg.leaf"]));
    assert_eq!(
        leaf["path"],
        serde_json::json!(["pkg.Alpha.run", "pkg.helper", "pkg.leaf"])
    );
}
