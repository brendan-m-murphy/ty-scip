use std::{
    env, fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use scip::types::{
    Document, Index, Occurrence, Relationship, SymbolInformation, SymbolRole, symbol_information,
};
use scip_graphify::graphify::to_json;

fn definition(symbol: &str, line: i32, end_line: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range: vec![line, 0, 6],
        enclosing_range: vec![line, 0, end_line, 0],
        symbol_roles: SymbolRole::Definition as i32,
        ..Default::default()
    }
}

#[test]
#[ignore = "CI installs and pins graphifyy==0.9.55"]
fn graphify_0_9_55_accepts_projection_and_traverses_directionally() {
    let mut implementor = SymbolInformation {
        symbol: "example package 1.0 Impl#".into(),
        display_name: "Impl".into(),
        kind: symbol_information::Kind::Class.into(),
        ..Default::default()
    };
    implementor.relationships.push(Relationship {
        symbol: "example package 1.0 Base#".into(),
        is_implementation: true,
        ..Default::default()
    });
    let index = Index {
        documents: vec![Document {
            relative_path: "example.py".into(),
            symbols: vec![
                SymbolInformation {
                    symbol: "example package 1.0 owner().".into(),
                    display_name: "owner".into(),
                    kind: symbol_information::Kind::Function.into(),
                    ..Default::default()
                },
                SymbolInformation {
                    symbol: "example package 1.0 target().".into(),
                    display_name: "target".into(),
                    kind: symbol_information::Kind::Function.into(),
                    ..Default::default()
                },
                implementor,
                SymbolInformation {
                    symbol: "example package 1.0 Base#".into(),
                    display_name: "Base".into(),
                    kind: symbol_information::Kind::Class.into(),
                    ..Default::default()
                },
            ],
            occurrences: vec![
                definition("example package 1.0 owner().", 0, 2),
                Occurrence {
                    symbol: "example package 1.0 target().".into(),
                    range: vec![1, 4, 10],
                    symbol_roles: SymbolRole::ReadAccess as i32,
                    ..Default::default()
                },
                definition("example package 1.0 target().", 3, 4),
                definition("example package 1.0 Impl#", 5, 6),
                definition("example package 1.0 Base#", 7, 8),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = env::temp_dir().join(format!("scip-graphify-real-{nonce}"));
    fs::create_dir_all(&directory).expect("create temporary directory");
    let graph = directory.join("graph.json");
    fs::write(&graph, to_json(&index).expect("serialize graph")).expect("write graph");
    let binary = env::var_os("GRAPHIFY_BIN").unwrap_or_else(|| "graphify".into());

    let path = Command::new(&binary)
        .args(["path", "owner", "target", "--graph"])
        .arg(&graph)
        .output()
        .expect("run pinned Graphify path");
    assert!(
        path.status.success(),
        "{}",
        String::from_utf8_lossy(&path.stderr)
    );
    assert!(String::from_utf8_lossy(&path.stdout).contains("owner --references"));

    let affected = Command::new(&binary)
        .args([
            "affected",
            "Base",
            "--relation",
            "implements",
            "--depth",
            "1",
            "--graph",
        ])
        .arg(&graph)
        .output()
        .expect("run pinned Graphify affected");
    assert!(
        affected.status.success(),
        "{}",
        String::from_utf8_lossy(&affected.stderr)
    );
    assert!(String::from_utf8_lossy(&affected.stdout).contains("Impl [implements]"));

    fs::remove_dir_all(directory).expect("remove temporary directory");
}
