use std::collections::{BTreeMap, BTreeSet};

use scip::types::{
    Index, Occurrence, SymbolInformation, SymbolRole, occurrence, symbol_information,
};
use serde::Serialize;

const EXTERNAL_SOURCE: &str = "";
const EXTERNAL_SYMBOL_PATH: &str = "<external>";

/// Graphify-compatible graph document.
#[derive(Debug, Serialize)]
pub struct Graph {
    pub directed: bool,
    pub multigraph: bool,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub hyperedges: Vec<serde_json::Value>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// A source file or SCIP symbol.  The extra SCIP fields are intentionally
/// retained: Graphify ignores unknown fields, while clients can use them to
/// navigate back to the exact indexed evidence.
#[derive(Debug, Serialize, Clone)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub file_type: String,
    pub source_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_range: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scip_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scip_kind: Option<i32>,
    #[serde(rename = "_origin")]
    pub origin: String,
}

/// A directed Graphify edge with SCIP evidence and an explicit provenance.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub context: String,
    pub confidence: String,
    pub source_file: String,
    pub source_location: String,
    pub source_range: Vec<i32>,
    pub symbol_roles: i32,
    pub position_encoding: i32,
    pub weight: f64,
    pub provenance: String,
    #[serde(rename = "_origin")]
    pub origin: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Range {
    start_line: i32,
    start_character: i32,
    end_line: i32,
    end_character: i32,
}

#[derive(Clone, Debug, Default)]
struct SymbolMetadata {
    display_name: Option<String>,
    kind: Option<symbol_information::Kind>,
    kind_conflict: bool,
    enclosing_symbol: Option<String>,
    enclosing_conflict: bool,
}

impl SymbolMetadata {
    fn absorb(&mut self, info: &SymbolInformation) {
        if !info.display_name.is_empty()
            && self
                .display_name
                .as_ref()
                .is_none_or(|current| info.display_name < *current)
        {
            self.display_name = Some(info.display_name.clone());
        }
        if !self.kind_conflict
            && let Ok(kind) = info.kind.enum_value()
            && kind != symbol_information::Kind::UnspecifiedKind
        {
            match self.kind {
                None => self.kind = Some(kind),
                Some(current) if current == kind => {}
                Some(_) => {
                    self.kind = None;
                    self.kind_conflict = true;
                }
            }
        }
        if !self.enclosing_conflict && !info.enclosing_symbol.is_empty() {
            match self.enclosing_symbol.as_deref() {
                None => self.enclosing_symbol = Some(info.enclosing_symbol.clone()),
                Some(current) if current == info.enclosing_symbol => {}
                Some(_) => {
                    self.enclosing_symbol = None;
                    self.enclosing_conflict = true;
                }
            }
        }
    }
}

impl Range {
    fn parse(values: &[i32]) -> Option<Self> {
        match values {
            [line, start, end] => Some(Self {
                start_line: *line,
                start_character: *start,
                end_line: *line,
                end_character: *end,
            }),
            [start_line, start_character, end_line, end_character] => Some(Self {
                start_line: *start_line,
                start_character: *start_character,
                end_line: *end_line,
                end_character: *end_character,
            }),
            _ => None,
        }
    }

    fn contains(self, other: Self) -> bool {
        (self.start_line, self.start_character) <= (other.start_line, other.start_character)
            && (other.end_line, other.end_character) <= (self.end_line, self.end_character)
    }

    fn size(self) -> (i32, i32, i32, i32) {
        (
            self.end_line - self.start_line,
            self.end_character - self.start_character,
            self.end_line,
            self.end_character,
        )
    }
}

/// Normalize SCIP's typed ranges, falling back to the deprecated vectors for
/// older producers. Typed ranges intentionally win when both are present.
fn normalized_range(occurrence: &Occurrence, enclosing: bool) -> Option<(Range, Vec<i32>)> {
    let typed = if enclosing {
        occurrence
            .typed_enclosing_range
            .as_ref()
            .and_then(|range| match range {
                occurrence::Typed_enclosing_range::SingleLineEnclosingRange(range) => Some((
                    Range {
                        start_line: range.line,
                        start_character: range.start_character,
                        end_line: range.line,
                        end_character: range.end_character,
                    },
                    vec![range.line, range.start_character, range.end_character],
                )),
                occurrence::Typed_enclosing_range::MultiLineEnclosingRange(range) => Some((
                    Range {
                        start_line: range.start_line,
                        start_character: range.start_character,
                        end_line: range.end_line,
                        end_character: range.end_character,
                    },
                    vec![
                        range.start_line,
                        range.start_character,
                        range.end_line,
                        range.end_character,
                    ],
                )),
                _ => None,
            })
    } else {
        occurrence
            .typed_range
            .as_ref()
            .and_then(|range| match range {
                occurrence::Typed_range::SingleLineRange(range) => Some((
                    Range {
                        start_line: range.line,
                        start_character: range.start_character,
                        end_line: range.line,
                        end_character: range.end_character,
                    },
                    vec![range.line, range.start_character, range.end_character],
                )),
                occurrence::Typed_range::MultiLineRange(range) => Some((
                    Range {
                        start_line: range.start_line,
                        start_character: range.start_character,
                        end_line: range.end_line,
                        end_character: range.end_character,
                    },
                    vec![
                        range.start_line,
                        range.start_character,
                        range.end_line,
                        range.end_character,
                    ],
                )),
                _ => None,
            })
    };
    typed.or_else(|| {
        if enclosing {
            if !occurrence.enclosing_range.is_empty() {
                return Range::parse(&occurrence.enclosing_range)
                    .map(|range| (range, occurrence.enclosing_range.clone()));
            }
            normalized_range(occurrence, false)
        } else {
            Range::parse(&occurrence.range).map(|range| (range, occurrence.range.clone()))
        }
    })
}

/// Convert an in-memory SCIP index to a deterministic Graphify graph.
pub fn to_graph(index: &Index) -> Graph {
    let mut nodes = BTreeMap::<String, Node>::new();
    let mut edges = BTreeMap::<String, Edge>::new();

    let mut symbol_info = BTreeMap::<String, SymbolMetadata>::new();
    let mut local_symbol_info = BTreeMap::<String, BTreeMap<String, SymbolMetadata>>::new();
    for document in &index.documents {
        for info in &document.symbols {
            if info.symbol.starts_with("local ") {
                local_symbol_info
                    .entry(document.relative_path.clone())
                    .or_default()
                    .entry(info.symbol.clone())
                    .or_default()
                    .absorb(info);
            } else {
                symbol_info
                    .entry(info.symbol.clone())
                    .or_default()
                    .absorb(info);
            }
        }
    }
    for info in &index.external_symbols {
        if !info.symbol.starts_with("local ") {
            symbol_info
                .entry(info.symbol.clone())
                .or_default()
                .absorb(info);
        }
    }

    // Build file nodes first, in key order.  BTreeMap sorting also makes the
    // output independent of protobuf insertion order.
    for document in &index.documents {
        let path = document.relative_path.clone();
        let id = file_id(&path);
        nodes.entry(id.clone()).or_insert_with(|| Node {
            id,
            label: path.rsplit('/').next().unwrap_or(&path).to_owned(),
            file_type: "code".to_owned(),
            source_file: path,
            source_location: Some("L1".to_owned()),
            source_range: None,
            scip_symbol: None,
            scip_kind: None,
            origin: "scip".to_owned(),
        });
    }

    // Collect all definitions and their ranges.  This permits a reference to
    // be owned by the smallest enclosing definition, without losing a local
    // symbol whose SymbolInformation is absent from the document.
    let mut definitions = BTreeMap::<String, Vec<(Range, Vec<i32>, String, i32)>>::new();
    for document in &index.documents {
        for occurrence in &document.occurrences {
            if occurrence.symbol_roles & SymbolRole::Definition as i32 == 0 {
                continue;
            }
            // The occurrence range identifies the definition token. SCIP's
            // enclosing range identifies its body and is the range used for
            // assigning references and nested definitions to their owner.
            let Some((range, _)) = normalized_range(occurrence, true) else {
                continue;
            };
            let Some((_, raw_range)) = normalized_range(occurrence, false) else {
                continue;
            };
            let symbol = occurrence.symbol.clone();
            let id = symbol_id(&document.relative_path, &symbol);
            let info = symbol_metadata_for(
                &document.relative_path,
                &symbol_info,
                &local_symbol_info,
                &symbol,
            );
            let label = display_name(&symbol, info);
            add_symbol_node(
                &mut nodes,
                &id,
                label,
                &document.relative_path,
                &raw_range,
                &symbol,
                info,
                "scip",
            );
            definitions
                .entry(document.relative_path.clone())
                .or_default()
                .push((range, raw_range, symbol, occurrence.symbol_roles));
        }
    }

    // Include symbols represented only by SymbolInformation. Local SCIP
    // symbols are document-scoped, so this must iterate documents rather than
    // using the symbol string as a global key.
    for document in &index.documents {
        for info in &document.symbols {
            let id = symbol_id(&document.relative_path, &info.symbol);
            let preferred = symbol_metadata_for(
                &document.relative_path,
                &symbol_info,
                &local_symbol_info,
                &info.symbol,
            );
            ensure_target_node(
                &mut nodes,
                &id,
                &document.relative_path,
                &info.symbol,
                preferred,
            );
        }
    }
    for info in &index.external_symbols {
        let id = symbol_id(EXTERNAL_SYMBOL_PATH, &info.symbol);
        ensure_target_node(
            &mut nodes,
            &id,
            EXTERNAL_SOURCE,
            &info.symbol,
            symbol_info.get(&info.symbol),
        );
    }

    // Every definition is contained by its source file.  Definition ranges
    // are retained on the edge, rather than reduced to a source chunk.
    for document in &index.documents {
        let file = file_id(&document.relative_path);
        let mut defs = definitions
            .get(&document.relative_path)
            .cloned()
            .unwrap_or_default();
        defs.sort_by(|left, right| (&left.0, &left.2).cmp(&(&right.0, &right.2)));
        for (range, raw_range, symbol, roles) in &defs {
            let target = symbol_id(&document.relative_path, symbol);
            insert_edge(
                &mut edges,
                edge(
                    file.clone(),
                    target.clone(),
                    "contains",
                    "definition",
                    &document.relative_path,
                    raw_range,
                    *roles,
                    document.position_encoding.value(),
                    "DERIVED",
                ),
            );

            // Preserve symbol hierarchy as a derived edge. The smallest
            // enclosing definition owns a nested definition just as it owns
            // references in its body.
            let explicit_parent = symbol_metadata_for(
                &document.relative_path,
                &symbol_info,
                &local_symbol_info,
                symbol,
            )
            .and_then(|info| info.enclosing_symbol.clone())
            .filter(|parent| {
                symbol_metadata_for(
                    &document.relative_path,
                    &symbol_info,
                    &local_symbol_info,
                    parent,
                )
                .is_some()
            });
            let parent = explicit_parent.or_else(|| {
                defs.iter()
                    .filter(|(candidate, _, candidate_symbol, _)| {
                        candidate_symbol != symbol
                            && candidate.contains(*range)
                            && is_lexical_scope(
                                candidate_symbol,
                                &document.relative_path,
                                &symbol_info,
                                &local_symbol_info,
                            )
                    })
                    .min_by_key(|(candidate, _, candidate_symbol, _)| {
                        (candidate.size(), candidate_symbol.clone())
                    })
                    .map(|(_, _, parent, _)| parent.clone())
            });
            if let Some(parent) = parent {
                if !defs.iter().any(|(_, _, candidate, _)| candidate == &parent) {
                    let parent_id = symbol_id(&document.relative_path, &parent);
                    ensure_target_node(
                        &mut nodes,
                        &parent_id,
                        &document.relative_path,
                        &parent,
                        symbol_metadata_for(
                            &document.relative_path,
                            &symbol_info,
                            &local_symbol_info,
                            &parent,
                        ),
                    );
                }
                insert_edge(
                    &mut edges,
                    edge(
                        symbol_id(&document.relative_path, &parent),
                        target.clone(),
                        "contains",
                        "method",
                        &document.relative_path,
                        raw_range,
                        *roles,
                        document.position_encoding.value(),
                        "DERIVED",
                    ),
                );
            }
        }
    }

    for document in &index.documents {
        let mut defs = definitions
            .get(&document.relative_path)
            .cloned()
            .unwrap_or_default();
        defs.sort_by_key(|(range, _, symbol, _)| (range.size(), symbol.clone()));

        for occurrence in &document.occurrences {
            if occurrence.symbol.is_empty()
                || occurrence.symbol_roles & SymbolRole::Definition as i32 != 0
            {
                continue;
            }
            let Some((range, raw_range)) = normalized_range(occurrence, false) else {
                continue;
            };
            let target = symbol_id(&document.relative_path, &occurrence.symbol);
            let target_source = if occurrence.symbol.starts_with("local ") {
                document.relative_path.as_str()
            } else {
                EXTERNAL_SOURCE
            };
            ensure_target_node(
                &mut nodes,
                &target,
                target_source,
                &occurrence.symbol,
                symbol_metadata_for(
                    &document.relative_path,
                    &symbol_info,
                    &local_symbol_info,
                    &occurrence.symbol,
                ),
            );
            let owner = defs
                .iter()
                .filter(|(definition, _, symbol, _)| {
                    definition.contains(range)
                        && is_lexical_scope(
                            symbol,
                            &document.relative_path,
                            &symbol_info,
                            &local_symbol_info,
                        )
                })
                .min_by_key(|(definition, _, symbol, _)| (definition.size(), symbol.clone()))
                .map(|(_, _, symbol, _)| symbol_id(&document.relative_path, symbol))
                .unwrap_or_else(|| file_id(&document.relative_path));
            let (relation, context) = relation_for(occurrence.symbol_roles);
            insert_edge(
                &mut edges,
                edge(
                    owner,
                    target,
                    relation,
                    context,
                    &document.relative_path,
                    &raw_range,
                    occurrence.symbol_roles,
                    document.position_encoding.value(),
                    "SCIP",
                ),
            );
        }
    }

    // Relationships are semantic SCIP evidence, and therefore are preserved
    // independently of whether the target has a local definition.
    let relationship_sources = index
        .documents
        .iter()
        .flat_map(|document| {
            document.symbols.iter().map(move |info| {
                (
                    Some(document),
                    document.relative_path.as_str(),
                    document.relative_path.as_str(),
                    document.position_encoding.value(),
                    info,
                )
            })
        })
        .chain(
            index
                .external_symbols
                .iter()
                .map(|info| (None, EXTERNAL_SYMBOL_PATH, EXTERNAL_SOURCE, 0, info)),
        );
    for (document, identity_path, source_file, position_encoding, info) in relationship_sources {
        let source = symbol_id(identity_path, &info.symbol);
        let source_info = document
            .and_then(|document| {
                symbol_metadata_for(
                    &document.relative_path,
                    &symbol_info,
                    &local_symbol_info,
                    &info.symbol,
                )
            })
            .or_else(|| symbol_info.get(&info.symbol));
        ensure_target_node(&mut nodes, &source, source_file, &info.symbol, source_info);
        for relationship in &info.relationships {
            let target_identity_path = if relationship.symbol.starts_with("local ") {
                identity_path
            } else {
                EXTERNAL_SYMBOL_PATH
            };
            let target_source_file = if relationship.symbol.starts_with("local ") {
                source_file
            } else {
                EXTERNAL_SOURCE
            };
            let target_info = document
                .and_then(|document| {
                    symbol_metadata_for(
                        &document.relative_path,
                        &symbol_info,
                        &local_symbol_info,
                        &relationship.symbol,
                    )
                })
                .or_else(|| symbol_info.get(&relationship.symbol));
            let target = symbol_id(target_identity_path, &relationship.symbol);
            ensure_target_node(
                &mut nodes,
                &target,
                target_source_file,
                &relationship.symbol,
                target_info,
            );
            let mut relations = Vec::new();
            if relationship.is_implementation {
                relations.push("implements");
            }
            if relationship.is_type_definition {
                relations.push("type_definition");
            }
            if relationship.is_reference {
                relations.push("references");
            }
            if relationship.is_definition {
                relations.push("relationship_definition");
            }
            for relation in relations {
                insert_edge(
                    &mut edges,
                    edge(
                        source.clone(),
                        target.clone(),
                        relation,
                        "relationship",
                        source_file,
                        &[],
                        0,
                        position_encoding,
                        "SCIP",
                    ),
                );
            }
        }
    }

    let graph = Graph {
        // Graphify's query command intentionally loads its persisted graph as
        // undirected context, while path/affected force directed traversal.
        // The stored source/target endpoints still carry the true direction.
        directed: false,
        multigraph: true,
        nodes: nodes.into_values().collect(),
        edges: edges.into_values().collect(),
        hyperedges: Vec::new(),
        input_tokens: 0,
        output_tokens: 0,
    };
    let node_ids: BTreeSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    debug_assert!(
        graph
            .edges
            .iter()
            .all(|edge| node_ids.contains(edge.source.as_str())
                && node_ids.contains(edge.target.as_str()))
    );
    graph
}

/// Serialize a SCIP index directly to Graphify JSON.
pub fn to_json(index: &Index) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&to_graph(index))
}

#[allow(clippy::too_many_arguments)]
fn add_symbol_node(
    nodes: &mut BTreeMap<String, Node>,
    id: &str,
    label: String,
    path: &str,
    raw_range: &[i32],
    symbol: &str,
    info: Option<&SymbolMetadata>,
    origin: &str,
) {
    nodes.entry(id.to_owned()).or_insert_with(|| Node {
        id: id.to_owned(),
        label,
        file_type: "code".to_owned(),
        source_file: path.to_owned(),
        source_location: source_location(raw_range),
        source_range: Some(raw_range.to_owned()),
        scip_symbol: Some(symbol.to_owned()),
        scip_kind: info.and_then(|item| item.kind.map(|kind| kind as i32)),
        origin: origin.to_owned(),
    });
}

fn ensure_target_node(
    nodes: &mut BTreeMap<String, Node>,
    id: &str,
    path: &str,
    symbol: &str,
    info: Option<&SymbolMetadata>,
) {
    if nodes.contains_key(id) {
        return;
    }
    nodes.insert(
        id.to_owned(),
        Node {
            id: id.to_owned(),
            label: display_name(symbol, info),
            file_type: "code".to_owned(),
            source_file: path.to_owned(),
            source_location: None,
            source_range: None,
            scip_symbol: Some(symbol.to_owned()),
            scip_kind: info.and_then(|item| item.kind.map(|kind| kind as i32)),
            origin: "scip".to_owned(),
        },
    );
}

fn insert_edge(edges: &mut BTreeMap<String, Edge>, edge: Edge) {
    let key = format!(
        "{}\0{}\0{}\0{}\0{}\0{:?}\0{}",
        edge.source,
        edge.target,
        edge.relation,
        edge.context,
        edge.source_file,
        edge.source_range,
        edge.symbol_roles
    );
    edges.entry(key).or_insert(edge);
}

#[allow(clippy::too_many_arguments)]
fn edge(
    source: String,
    target: String,
    relation: &str,
    context: &str,
    path: &str,
    raw_range: &[i32],
    symbol_roles: i32,
    position_encoding: i32,
    provenance: &str,
) -> Edge {
    Edge {
        source,
        target,
        relation: relation.to_owned(),
        context: context.to_owned(),
        confidence: "EXTRACTED".to_owned(),
        source_file: path.to_owned(),
        source_location: source_location(raw_range).unwrap_or_else(|| "".to_owned()),
        source_range: raw_range.to_owned(),
        symbol_roles,
        position_encoding,
        weight: 1.0,
        provenance: provenance.to_owned(),
        origin: provenance.to_ascii_lowercase(),
    }
}

fn relation_for(roles: i32) -> (&'static str, &'static str) {
    if roles & SymbolRole::Import as i32 != 0 {
        ("imports", "import")
    } else if roles & SymbolRole::WriteAccess as i32 != 0 {
        ("references", "write")
    } else if roles & SymbolRole::ReadAccess as i32 != 0 {
        ("references", "read")
    } else {
        ("references", "reference")
    }
}

fn source_location(raw_range: &[i32]) -> Option<String> {
    let range = Range::parse(raw_range)?;
    if range.start_line == range.end_line {
        Some(format!("L{}", range.start_line + 1))
    } else {
        Some(format!("L{}-L{}", range.start_line + 1, range.end_line + 1))
    }
}

fn file_id(path: &str) -> String {
    format!("file:{path}")
}

fn symbol_metadata_for<'a>(
    path: &str,
    global: &'a BTreeMap<String, SymbolMetadata>,
    local: &'a BTreeMap<String, BTreeMap<String, SymbolMetadata>>,
    symbol: &str,
) -> Option<&'a SymbolMetadata> {
    if symbol.starts_with("local ") {
        local.get(path).and_then(|symbols| symbols.get(symbol))
    } else {
        global.get(symbol)
    }
}

fn is_lexical_scope(
    symbol: &str,
    path: &str,
    global: &BTreeMap<String, SymbolMetadata>,
    local: &BTreeMap<String, BTreeMap<String, SymbolMetadata>>,
) -> bool {
    // Local bindings (imports, parameters, variables) are not scopes, while
    // local nested callables and types can be valid lexical scopes.
    let Some(info) = symbol_metadata_for(path, global, local, symbol) else {
        return false;
    };
    let Some(kind) = info.kind else {
        return false;
    };
    if symbol.starts_with("local ") {
        matches!(
            kind,
            symbol_information::Kind::Class
                | symbol_information::Kind::Method
                | symbol_information::Kind::Function
                | symbol_information::Kind::Constructor
                | symbol_information::Kind::Property
        )
    } else {
        matches!(
            kind,
            symbol_information::Kind::Module
                | symbol_information::Kind::Class
                | symbol_information::Kind::Method
                | symbol_information::Kind::Function
                | symbol_information::Kind::Constructor
                | symbol_information::Kind::Property
        )
    }
}

/// SCIP local symbols are only unique within a document, so their path is
/// part of the graph identity. Global symbols deliberately retain one shared
/// identity across documents and external-symbol records.
fn symbol_id(path: &str, symbol: &str) -> String {
    if symbol.starts_with("local ") {
        format!("symbol:{path}:{symbol}")
    } else {
        format!("symbol:{symbol}")
    }
}

fn display_name(symbol: &str, info: Option<&SymbolMetadata>) -> String {
    if let Some(name) = info.and_then(|item| item.display_name.as_deref()) {
        if symbol.starts_with("local ") {
            return format!("{name} [{symbol}]");
        }
        return name.to_owned();
    }
    symbol
        .rsplit(['/', '#', '.', '(', ')'])
        .find(|part| !part.is_empty())
        .unwrap_or(symbol)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::{Document, Index, Occurrence, SymbolInformation};

    fn occurrence(symbol: &str, range: Vec<i32>, roles: i32) -> Occurrence {
        Occurrence {
            symbol: symbol.to_owned(),
            range,
            symbol_roles: roles,
            ..Default::default()
        }
    }

    #[test]
    fn local_ids_include_document_path() {
        let index = Index {
            documents: vec![Document {
                relative_path: "a.py".into(),
                occurrences: vec![occurrence(
                    "local 0",
                    vec![0, 0, 1],
                    SymbolRole::Definition as i32,
                )],
                ..Default::default()
            }],
            ..Default::default()
        };
        let graph = to_graph(&index);
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "symbol:a.py:local 0")
        );
    }

    #[test]
    fn reference_is_owned_by_smallest_definition() {
        let index = Index {
            documents: vec![Document {
                relative_path: "a.py".into(),
                symbols: vec![
                    SymbolInformation {
                        symbol: "outer".into(),
                        kind: symbol_information::Kind::Function.into(),
                        ..Default::default()
                    },
                    SymbolInformation {
                        symbol: "inner".into(),
                        kind: symbol_information::Kind::Function.into(),
                        ..Default::default()
                    },
                ],
                occurrences: vec![
                    occurrence("outer", vec![0, 0, 5, 0], SymbolRole::Definition as i32),
                    occurrence("inner", vec![1, 0, 3, 0], SymbolRole::Definition as i32),
                    occurrence("target", vec![2, 4, 2, 10], SymbolRole::ReadAccess as i32),
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
                .any(|edge| edge.source == "symbol:inner" && edge.target == "symbol:target")
        );
    }

    #[test]
    fn equal_start_ranges_compare_endpoints_independently() {
        let outer = Range {
            start_line: 1,
            start_character: 5,
            end_line: 2,
            end_character: 0,
        };
        let inner = Range {
            start_line: 1,
            start_character: 5,
            end_line: 1,
            end_character: 8,
        };
        assert!(outer.contains(inner));
    }
}
