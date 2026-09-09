use scip::types::{
    Document, Index, MultiLineRange, Occurrence, Relationship, SymbolInformation, SymbolRole,
    symbol_information,
};
use ty_scip::graphify::{to_graph, to_json};

fn occurrence(symbol: &str, range: &[i32], roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.to_owned(),
        range: range.to_vec(),
        symbol_roles: roles,
        ..Default::default()
    }
}

fn definition_with_body(symbol: &str, token: &[i32], body: &[i32]) -> Occurrence {
    Occurrence {
        symbol: symbol.to_owned(),
        range: token.to_vec(),
        enclosing_range: body.to_vec(),
        symbol_roles: SymbolRole::Definition as i32,
        ..Default::default()
    }
}

fn typed_definition(symbol: &str) -> Occurrence {
    let mut occurrence = definition_with_body(symbol, &[99, 99, 99], &[99, 99, 99]);
    occurrence.set_multi_line_range(MultiLineRange {
        start_line: 0,
        start_character: 4,
        end_line: 0,
        end_character: 7,
        ..Default::default()
    });
    occurrence.set_multi_line_enclosing_range(MultiLineRange {
        start_line: 0,
        start_character: 0,
        end_line: 3,
        end_character: 0,
        ..Default::default()
    });
    occurrence
}

#[test]
fn emits_files_symbols_and_exact_reference_roles() {
    let index = Index {
        documents: vec![Document {
            relative_path: "pkg/main.py".into(),
            occurrences: vec![
                occurrence("pkg/main().", &[0, 0, 2, 0], SymbolRole::Definition as i32),
                occurrence("pkg/value.", &[1, 4, 1, 9], SymbolRole::Import as i32),
                occurrence("pkg/value.", &[2, 4, 2, 9], SymbolRole::ReadAccess as i32),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let graph = to_graph(&index);
    assert!(graph.nodes.iter().any(|node| node.id == "file:pkg/main.py"));
    assert!(graph.edges.iter().any(|edge| {
        edge.relation == "imports"
            && edge.context == "import"
            && edge.provenance == "SCIP"
            && edge.source_range == [1, 4, 1, 9]
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.relation == "references" && edge.context == "read" && edge.source_range == [2, 4, 2, 9]
    }));
}

#[test]
fn relationships_become_inherits_edges() {
    let index = Index {
        documents: vec![Document {
            relative_path: "pkg/types.py".into(),
            symbols: vec![SymbolInformation {
                symbol: "pkg/Dog#".into(),
                display_name: "Dog".into(),
                relationships: vec![Relationship {
                    symbol: "pkg/Animal#".into(),
                    is_implementation: true,
                    is_type_definition: true,
                    is_reference: true,
                    is_definition: true,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            occurrences: vec![occurrence(
                "pkg/Dog#",
                &[0, 0, 0, 3],
                SymbolRole::Definition as i32,
            )],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(graph.edges.iter().any(|edge| {
        edge.relation == "inherits"
            && edge.source == "symbol:pkg/Dog#"
            && edge.target == "symbol:pkg/Animal#"
    }));
    for relation in [
        "type_definition",
        "relationship_reference",
        "relationship_definition",
    ] {
        assert!(graph.edges.iter().any(|edge| {
            edge.relation == relation
                && edge.source == "symbol:pkg/Dog#"
                && edge.target == "symbol:pkg/Animal#"
        }));
    }
}

#[test]
fn json_is_deterministic_and_graphify_shaped() {
    let index = Index {
        documents: vec![
            Document {
                relative_path: "b.py".into(),
                ..Default::default()
            },
            Document {
                relative_path: "a.py".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let first = to_json(&index).unwrap();
    let second = to_json(&index).unwrap();
    assert_eq!(first, second);
    let graph = to_graph(&index);
    assert!(!graph.directed);
    assert!(graph.multigraph);
    assert!(first.contains("\"nodes\""));
    assert!(first.contains("\"edges\""));
    assert!(first.contains("\"hyperedges\""));
}

#[test]
fn same_local_symbol_in_two_documents_has_distinct_nodes_and_closed_edges() {
    let index = Index {
        documents: vec![
            Document {
                relative_path: "a.py".into(),
                occurrences: vec![
                    occurrence("local 0", &[0, 0, 1], SymbolRole::Definition as i32),
                    occurrence("local 0", &[1, 0, 1], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
            Document {
                relative_path: "b.py".into(),
                occurrences: vec![
                    occurrence("local 0", &[0, 0, 1], SymbolRole::Definition as i32),
                    occurrence("local 0", &[1, 0, 1], SymbolRole::ReadAccess as i32),
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let graph = to_graph(&index);
    let ids: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.id.contains("local 0"))
        .map(|node| node.id.as_str())
        .collect();
    assert_eq!(ids, vec!["symbol:a.py:local 0", "symbol:b.py:local 0"]);
    assert!(graph.edges.iter().all(|edge| {
        graph.nodes.iter().any(|node| node.id == edge.source)
            && graph.nodes.iter().any(|node| node.id == edge.target)
    }));
    assert!(
        graph
            .nodes
            .iter()
            .all(|node| { !node.source_file.is_empty() && node.file_type == "code" })
    );
}

#[test]
fn local_symbol_metadata_is_document_scoped() {
    let index = Index {
        documents: vec![
            Document {
                relative_path: "a.py".into(),
                symbols: vec![SymbolInformation {
                    symbol: "local 0".into(),
                    display_name: "alpha".into(),
                    kind: symbol_information::Kind::Variable.into(),
                    ..Default::default()
                }],
                occurrences: vec![occurrence(
                    "local 0",
                    &[0, 0, 1],
                    SymbolRole::Definition as i32,
                )],
                ..Default::default()
            },
            Document {
                relative_path: "b.py".into(),
                symbols: vec![SymbolInformation {
                    symbol: "local 0".into(),
                    display_name: "beta".into(),
                    kind: symbol_information::Kind::Parameter.into(),
                    ..Default::default()
                }],
                occurrences: vec![occurrence(
                    "local 0",
                    &[0, 0, 1],
                    SymbolRole::Definition as i32,
                )],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let graph = to_graph(&index);
    let alpha = graph
        .nodes
        .iter()
        .find(|node| node.id == "symbol:a.py:local 0")
        .unwrap();
    let beta = graph
        .nodes
        .iter()
        .find(|node| node.id == "symbol:b.py:local 0")
        .unwrap();
    assert_eq!(alpha.label, "alpha [local 0]");
    assert_eq!(beta.label, "beta [local 0]");
    assert_eq!(
        alpha.scip_kind,
        Some(symbol_information::Kind::Variable as i32)
    );
    assert_eq!(
        beta.scip_kind,
        Some(symbol_information::Kind::Parameter as i32)
    );
}

#[test]
fn references_use_definition_enclosing_range_for_ownership() {
    let index = Index {
        documents: vec![Document {
            relative_path: "owner.py".into(),
            symbols: vec![SymbolInformation {
                symbol: "owner".into(),
                kind: symbol_information::Kind::Function.into(),
                ..Default::default()
            }],
            occurrences: vec![
                definition_with_body("owner", &[0, 4, 7], &[0, 0, 3, 0]),
                occurrence("target", &[2, 2, 2, 8], SymbolRole::ReadAccess as i32),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(graph.edges.iter().any(|edge| {
        edge.source == "symbol:owner"
            && edge.target == "symbol:target"
            && edge.relation == "references"
    }));
    let definition = graph
        .nodes
        .iter()
        .find(|node| node.id == "symbol:owner")
        .unwrap();
    assert_eq!(definition.source_range.as_deref(), Some(&[0, 4, 7][..]));
}

#[test]
fn typed_ranges_take_precedence_over_conflicting_legacy_ranges() {
    let mut reference = occurrence("target", &[88, 88, 88], SymbolRole::ReadAccess as i32);
    reference.set_multi_line_range(MultiLineRange {
        start_line: 2,
        start_character: 2,
        end_line: 2,
        end_character: 8,
        ..Default::default()
    });
    let index = Index {
        documents: vec![Document {
            relative_path: "typed.py".into(),
            symbols: vec![SymbolInformation {
                symbol: "owner".into(),
                kind: symbol_information::Kind::Function.into(),
                ..Default::default()
            }],
            occurrences: vec![typed_definition("owner"), reference],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    let definition = graph
        .nodes
        .iter()
        .find(|node| node.id == "symbol:owner")
        .unwrap();
    assert_eq!(definition.source_range.as_deref(), Some(&[0, 4, 0, 7][..]));
    assert!(graph.edges.iter().any(|edge| {
        edge.source == "symbol:owner"
            && edge.target == "symbol:target"
            && edge.source_range == [2, 2, 2, 8]
    }));
}

#[test]
fn typed_token_range_owns_definition_when_typed_enclosing_range_is_missing() {
    let mut definition = occurrence("owner", &[99, 99, 99], SymbolRole::Definition as i32);
    definition.set_multi_line_range(MultiLineRange {
        start_line: 0,
        start_character: 4,
        end_line: 0,
        end_character: 7,
        ..Default::default()
    });
    let mut reference = occurrence("target", &[99, 99, 99], SymbolRole::ReadAccess as i32);
    reference.set_multi_line_range(MultiLineRange {
        start_line: 0,
        start_character: 5,
        end_line: 0,
        end_character: 6,
        ..Default::default()
    });
    let index = Index {
        documents: vec![Document {
            relative_path: "typed-only.py".into(),
            symbols: vec![SymbolInformation {
                symbol: "owner".into(),
                kind: symbol_information::Kind::Function.into(),
                ..Default::default()
            }],
            occurrences: vec![definition, reference],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(graph.nodes.iter().any(|node| node.id == "symbol:owner"));
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| { edge.source == "symbol:owner" && edge.target == "symbol:target" })
    );
}

#[test]
fn local_import_definition_does_not_own_module_references() {
    let index = Index {
        documents: vec![Document {
            relative_path: "module.py".into(),
            symbols: vec![
                SymbolInformation {
                    symbol: "pkg/module.".into(),
                    display_name: "module".into(),
                    kind: symbol_information::Kind::Module.into(),
                    ..Default::default()
                },
                SymbolInformation {
                    symbol: "local 0".into(),
                    display_name: "imported".into(),
                    // A local import can carry Module kind, but it remains a
                    // binding rather than a lexical scope.
                    kind: symbol_information::Kind::Module.into(),
                    ..Default::default()
                },
            ],
            occurrences: vec![
                definition_with_body("pkg/module.", &[0, 0, 6], &[0, 0, 3, 0]),
                definition_with_body("local 0", &[1, 0, 8], &[0, 0, 3, 0]),
                occurrence("target", &[2, 0, 6], SymbolRole::ReadAccess as i32),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| { edge.source == "symbol:pkg/module." && edge.target == "symbol:target" })
    );
    assert!(!graph.edges.iter().any(|edge| {
        edge.source == "symbol:module.py:local 0" && edge.target == "symbol:target"
    }));
}

#[test]
fn local_callable_scopes_and_explicit_enclosing_symbol_are_preserved() {
    let mut function_info = SymbolInformation {
        symbol: "local 0".into(),
        display_name: "nested_fn".into(),
        kind: symbol_information::Kind::Function.into(),
        ..Default::default()
    };
    function_info.enclosing_symbol = "pkg/module.".into();
    let mut class_info = SymbolInformation {
        symbol: "local 1".into(),
        display_name: "NestedClass".into(),
        kind: symbol_information::Kind::Class.into(),
        ..Default::default()
    };
    class_info.enclosing_symbol = "pkg/module.".into();
    let index = Index {
        documents: vec![Document {
            relative_path: "nested.py".into(),
            symbols: vec![
                SymbolInformation {
                    symbol: "pkg/module.".into(),
                    kind: symbol_information::Kind::Module.into(),
                    ..Default::default()
                },
                function_info,
                class_info,
            ],
            occurrences: vec![
                definition_with_body("pkg/module.", &[0, 0, 6], &[0, 0, 5, 0]),
                definition_with_body("local 0", &[1, 0, 8], &[1, 0, 2, 0]),
                definition_with_body("local 1", &[3, 0, 10], &[3, 0, 4, 0]),
                occurrence("target", &[1, 5, 1, 11], SymbolRole::ReadAccess as i32),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(graph.edges.iter().any(|edge| {
        edge.source == "symbol:nested.py:local 0"
            && edge.target == "symbol:target"
            && edge.relation == "references"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.source == "symbol:pkg/module." && edge.target == "symbol:nested.py:local 0"
    }));
}

#[test]
fn explicit_parent_without_definition_occurrence_gets_direct_containment() {
    let mut child = SymbolInformation {
        symbol: "pkg/child().".into(),
        display_name: "child".into(),
        kind: symbol_information::Kind::Function.into(),
        ..Default::default()
    };
    child.enclosing_symbol = "pkg/metadata.".into();
    let index = Index {
        documents: vec![Document {
            relative_path: "metadata.py".into(),
            symbols: vec![
                SymbolInformation {
                    symbol: "pkg/metadata.".into(),
                    display_name: "metadata".into(),
                    kind: symbol_information::Kind::Module.into(),
                    ..Default::default()
                },
                child,
            ],
            occurrences: vec![definition_with_body(
                "pkg/child().",
                &[1, 0, 5],
                &[1, 0, 2, 0],
            )],
            ..Default::default()
        }],
        ..Default::default()
    };
    let graph = to_graph(&index);
    assert!(graph.edges.iter().any(|edge| {
        edge.source == "symbol:pkg/metadata."
            && edge.target == "symbol:pkg/child()."
            && edge.relation == "contains"
    }));
}
