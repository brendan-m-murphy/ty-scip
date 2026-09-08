use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use ruff_db::{
    files::File,
    parsed::parsed_module,
    source::source_text,
    system::{OsSystem, SystemPathBuf},
};
use ruff_python_ast::{
    AnyNodeRef, Identifier,
    visitor::source_order::{SourceOrderVisitor, TraversalSignal},
};
use ruff_text_size::{Ranged, TextRange};
use scip::{
    symbol::{format_symbol, parse_symbol},
    types::{
        Descriptor, Document, Index, Metadata, MultiLineRange, Occurrence, Package,
        PositionEncoding, ProtocolVersion, SingleLineRange, Symbol, SymbolInformation, SymbolRole,
        TextEncoding, ToolInfo, descriptor, symbol_information,
    },
    write_message_to_file,
};
use ty_ide::{HierarchicalSymbols, SymbolId, SymbolInfo, SymbolKind};
use ty_module_resolver::file_to_module;
use ty_project::{Db as _, ProjectDatabase, ProjectMetadata, SemanticDb as _};

#[derive(Default)]
struct IdentifierRanges(Vec<TextRange>);

impl<'ast> SourceOrderVisitor<'ast> for IdentifierRanges {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        if let AnyNodeRef::ExprName(name) = node {
            self.0.push(name.range());
        }
        TraversalSignal::Traverse
    }

    fn visit_identifier(&mut self, identifier: &'ast Identifier) {
        self.0.push(identifier.range());
    }
}

struct ParameterSymbols<'symbols> {
    callables: Vec<Option<Symbol>>,
    globals: &'symbols mut HashMap<TextRange, SymbolData>,
}

impl<'ast> SourceOrderVisitor<'ast> for ParameterSymbols<'_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::StmtFunctionDef(function) => self.callables.push(
                self.globals
                    .get(&function.name.range())
                    .map(|data| parse_symbol(&data.symbol).expect("ty-scip formatted symbol")),
            ),
            AnyNodeRef::ExprLambda(_) => self.callables.push(None),
            AnyNodeRef::Parameter(parameter) => {
                if let Some(Some(parent)) = self.callables.last() {
                    let mut symbol = parent.clone();
                    symbol.descriptors.push(Descriptor {
                        name: parameter.name.to_string(),
                        suffix: descriptor::Suffix::Parameter.into(),
                        ..Default::default()
                    });
                    self.globals.insert(
                        parameter.name.range(),
                        SymbolData {
                            symbol: format_symbol(symbol),
                            display_name: parameter.name.to_string(),
                            kind: symbol_information::Kind::Parameter,
                            full_range: parameter.range(),
                        },
                    );
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }

    fn leave_node(&mut self, node: AnyNodeRef<'ast>) {
        if matches!(
            node,
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::ExprLambda(_)
        ) {
            self.callables.pop();
        }
    }
}

#[derive(Clone)]
struct SymbolData {
    symbol: String,
    display_name: String,
    kind: symbol_information::Kind,
    full_range: TextRange,
}

struct FileData {
    file: File,
    relative_path: String,
    source: String,
    globals: HashMap<TextRange, SymbolData>,
    locals: HashMap<TextRange, SymbolData>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Edge {
    source_file: usize,
    source_range: TextRange,
    target_file: usize,
    target_range: TextRange,
}

fn main() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let root = arguments
        .next()
        .map_or_else(env::current_dir, |path| PathBuf::from(path).canonicalize());
    let output = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Err("usage: ty-scip [PROJECT_ROOT] [OUTPUT.scip]".into());
    }

    let discovery_root = root.map_err(|error| error.to_string())?;
    let system_root = SystemPathBuf::from_path_buf(discovery_root)
        .map_err(|path| format!("project path is not UTF-8: {}", path.display()))?;
    let system = OsSystem::new(&system_root);
    let metadata =
        ProjectMetadata::discover(&system_root, &system).map_err(|error| error.to_string())?;
    let db = ProjectDatabase::fallible(metadata, system).map_err(|error| error.to_string())?;
    let root = db.project().root(&db).as_std_path().to_path_buf();

    let mut files = db.project().files(&db).iter().collect::<Vec<_>>();
    files.sort_by_key(|file| file.path(&db).to_string());
    let file_indices = files
        .iter()
        .enumerate()
        .map(|(index, file)| (*file, index))
        .collect::<HashMap<_, _>>();
    let mut data = Vec::with_capacity(files.len());
    for file in files {
        let path = file
            .path(&db)
            .as_system_path()
            .ok_or_else(|| format!("project file is not on disk: {}", file.path(&db)))?;
        let relative_path = path
            .as_std_path()
            .strip_prefix(&root)
            .map_err(|_| format!("project file is outside root: {path}"))?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let source = source_text(&db, file).as_str().to_owned();
        let program_file = db.program_file(file);
        let hierarchy = ty_ide::document_symbols(&db, program_file).to_hierarchical();
        let module = file_to_module(&db, program_file.resolver_file(&db))
            .ok_or_else(|| format!("project file has no importable module name: {path}"))?;
        let module_descriptors = module
            .name(&db)
            .components()
            .map(|name| Descriptor {
                name: name.to_owned(),
                suffix: descriptor::Suffix::Namespace.into(),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let module_name = module_descriptors
            .last()
            .map(|descriptor| descriptor.name.clone())
            .unwrap_or_default();
        let module_symbol = symbol_data(
            module_descriptors.clone(),
            module_name,
            symbol_information::Kind::Module,
            TextRange::new(0.into(), (source.len() as u32).into()),
        );
        let mut globals = HashMap::from([(TextRange::default(), module_symbol)]);
        for (id, info) in hierarchy.iter() {
            collect_global_symbols(&hierarchy, id, info, &module_descriptors, &mut globals);
        }
        {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            ParameterSymbols {
                callables: Vec::new(),
                globals: &mut globals,
            }
            .visit_body(module.suite());
        }
        data.push(FileData {
            file,
            relative_path,
            source,
            globals,
            locals: HashMap::new(),
        });
    }

    let mut edges = Vec::new();
    for (source_index, file_data) in data.iter().enumerate() {
        let program_file = db.program_file(file_data.file);
        let ranges = {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            let mut visitor = IdentifierRanges::default();
            visitor.visit_body(module.suite());
            visitor.0
        };

        for range in ranges {
            let Some(result) = ty_ide::goto_declaration(&db, program_file, range.start()) else {
                continue;
            };
            let mut targets = result
                .value
                .into_iter()
                .map(|target| (target.file(), target.focus_range()))
                .collect::<Vec<_>>();
            targets.sort_unstable_by_key(|(file, range)| {
                (file.path(&db).to_string(), range.start(), range.end())
            });
            targets.dedup();
            if let [(target_file, target_range)] = targets.as_slice()
                && let Some(target_file) = file_indices.get(target_file)
            {
                edges.push(Edge {
                    source_file: source_index,
                    source_range: range,
                    target_file: *target_file,
                    target_range: *target_range,
                });
            }
        }
    }
    edges.sort_unstable_by_key(|edge| {
        (
            edge.source_file,
            edge.source_range.start(),
            edge.source_range.end(),
        )
    });
    edges.dedup();

    for (target_file, file_data) in data.iter_mut().enumerate() {
        let mut ranges = edges
            .iter()
            .filter(|edge| {
                edge.target_file == target_file
                    && edge.source_file == target_file
                    && !file_data.globals.contains_key(&edge.target_range)
            })
            .map(|edge| edge.target_range)
            .collect::<Vec<_>>();
        ranges.sort_unstable_by_key(|range| (range.start(), range.end()));
        ranges.dedup();
        for (id, range) in ranges.into_iter().enumerate() {
            let display_name = source_slice(&file_data.source, range).to_owned();
            file_data.locals.insert(
                range,
                SymbolData {
                    symbol: format_symbol(Symbol::new_local(id)),
                    display_name,
                    kind: symbol_information::Kind::Variable,
                    full_range: range,
                },
            );
        }
    }

    let mut skipped_cross_file_locals = 0;
    for edge in &edges {
        let source = &data[edge.source_file];
        let target = &data[edge.target_file];
        let Some(symbol) = target
            .globals
            .get(&edge.target_range)
            .or_else(|| target.locals.get(&edge.target_range))
        else {
            continue;
        };
        if edge.source_file != edge.target_file && symbol.symbol.starts_with("local ") {
            skipped_cross_file_locals += 1;
            continue;
        }
        println!(
            "{}:{:?} -> {}:{:?}",
            source.relative_path, edge.source_range, target.relative_path, edge.target_range,
        );
    }
    if skipped_cross_file_locals > 0 {
        eprintln!("skipped {skipped_cross_file_locals} unsupported cross-file local targets");
    }

    if let Some(output) = output {
        write_index(&root, &output, &data, &edges)?;
    }
    Ok(())
}

fn collect_global_symbols(
    hierarchy: &HierarchicalSymbols,
    id: SymbolId,
    info: SymbolInfo<'_>,
    parents: &[Descriptor],
    output: &mut HashMap<TextRange, SymbolData>,
) {
    let (suffix, kind) = symbol_kinds(info.kind);
    let mut descriptors = parents.to_vec();
    descriptors.push(Descriptor {
        name: info.name.to_string(),
        suffix: suffix.into(),
        ..Default::default()
    });
    output.insert(
        info.name_range,
        symbol_data(
            descriptors.clone(),
            info.name.to_string(),
            kind,
            info.full_range,
        ),
    );
    if matches!(
        info.kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    ) {
        return;
    }
    for (child_id, child) in hierarchy.children(id) {
        collect_global_symbols(hierarchy, child_id, child, &descriptors, output);
    }
}

fn symbol_kinds(kind: SymbolKind) -> (descriptor::Suffix, symbol_information::Kind) {
    match kind {
        SymbolKind::Module => (
            descriptor::Suffix::Namespace,
            symbol_information::Kind::Module,
        ),
        SymbolKind::Class => (descriptor::Suffix::Type, symbol_information::Kind::Class),
        SymbolKind::Method => (descriptor::Suffix::Method, symbol_information::Kind::Method),
        SymbolKind::Function => (
            descriptor::Suffix::Method,
            symbol_information::Kind::Function,
        ),
        SymbolKind::Constructor => (
            descriptor::Suffix::Method,
            symbol_information::Kind::Constructor,
        ),
        SymbolKind::Parameter => (
            descriptor::Suffix::Parameter,
            symbol_information::Kind::Parameter,
        ),
        SymbolKind::TypeParameter => (
            descriptor::Suffix::TypeParameter,
            symbol_information::Kind::TypeParameter,
        ),
        SymbolKind::Variable => (descriptor::Suffix::Term, symbol_information::Kind::Variable),
        SymbolKind::Constant => (descriptor::Suffix::Term, symbol_information::Kind::Constant),
        SymbolKind::Property => (descriptor::Suffix::Term, symbol_information::Kind::Property),
        SymbolKind::Field => (descriptor::Suffix::Term, symbol_information::Kind::Field),
        SymbolKind::Import => (descriptor::Suffix::Term, symbol_information::Kind::Module),
    }
}

fn symbol_data(
    descriptors: Vec<Descriptor>,
    display_name: String,
    kind: symbol_information::Kind,
    full_range: TextRange,
) -> SymbolData {
    SymbolData {
        symbol: format_symbol(Symbol {
            scheme: "ty-scip".into(),
            package: Some(Package {
                manager: "python".into(),
                ..Default::default()
            })
            .into(),
            descriptors,
            ..Default::default()
        }),
        display_name,
        kind,
        full_range,
    }
}

fn write_index(
    root: &Path,
    output: &Path,
    data: &[FileData],
    edges: &[Edge],
) -> Result<(), String> {
    let mut documents = data
        .iter()
        .map(|file| Document {
            language: "python".into(),
            relative_path: file.relative_path.clone(),
            symbols: file
                .globals
                .values()
                .chain(file.locals.values())
                .map(|symbol| SymbolInformation {
                    symbol: symbol.symbol.clone(),
                    kind: symbol.kind.into(),
                    display_name: symbol.display_name.clone(),
                    ..Default::default()
                })
                .collect(),
            position_encoding: PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    for (document, file) in documents.iter_mut().zip(data) {
        let mut definitions = file.globals.iter().chain(&file.locals).collect::<Vec<_>>();
        definitions
            .sort_by_key(|(range, symbol)| (range.start(), range.end(), symbol.symbol.as_str()));
        document
            .occurrences
            .extend(definitions.into_iter().map(|(range, symbol)| {
                definition_occurrence(
                    &file.source,
                    *range,
                    symbol.symbol.clone(),
                    symbol.full_range,
                )
            }));
    }

    for edge in edges {
        if edge.source_file == edge.target_file && edge.source_range == edge.target_range {
            continue;
        }
        let target = &data[edge.target_file];
        let Some(symbol) = target
            .globals
            .get(&edge.target_range)
            .or_else(|| target.locals.get(&edge.target_range))
        else {
            continue;
        };
        if edge.source_file != edge.target_file && symbol.symbol.starts_with("local ") {
            continue;
        }
        documents[edge.source_file].occurrences.push(occurrence(
            &data[edge.source_file].source,
            edge.source_range,
            symbol.symbol.clone(),
            SymbolRole::ReadAccess as i32,
        ));
    }
    for document in &mut documents {
        document
            .symbols
            .sort_by(|left, right| left.symbol.cmp(&right.symbol));
        document
            .symbols
            .dedup_by(|left, right| left.symbol == right.symbol);
    }

    let index = Index {
        metadata: Some(Metadata {
            version: ProtocolVersion::UnspecifiedProtocolVersion.into(),
            tool_info: Some(ToolInfo {
                name: "ty-scip".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                arguments: env::args_os()
                    .map(|argument| argument.to_string_lossy().into_owned())
                    .collect(),
                ..Default::default()
            })
            .into(),
            project_root: file_uri(root),
            text_document_encoding: TextEncoding::UTF8.into(),
            ..Default::default()
        })
        .into(),
        documents,
        ..Default::default()
    };
    let temporary = output.with_extension("scip.tmp");
    write_message_to_file(&temporary, index).map_err(|error| error.to_string())?;
    fs::rename(&temporary, output).map_err(|error| error.to_string())
}

fn occurrence(source: &str, range: TextRange, symbol: String, roles: i32) -> Occurrence {
    // ponytail: linear offset conversion is enough for the spike; use Ruff's LineIndex if measured.
    let (line, start_character) = position(source, range.start().to_usize());
    let (end_line, end_character) = position(source, range.end().to_usize());
    debug_assert_eq!(line, end_line, "Python identifiers cannot span lines");
    let mut occurrence = Occurrence {
        range: vec![line, start_character, end_character],
        symbol,
        symbol_roles: roles,
        ..Default::default()
    };
    occurrence.set_single_line_range(SingleLineRange {
        line,
        start_character,
        end_character,
        ..Default::default()
    });
    occurrence
}

fn definition_occurrence(
    source: &str,
    range: TextRange,
    symbol: String,
    enclosing_range: TextRange,
) -> Occurrence {
    let mut occurrence = occurrence(source, range, symbol, SymbolRole::Definition as i32);
    let (start_line, start_character) = position(source, enclosing_range.start().to_usize());
    let (end_line, end_character) = position(source, enclosing_range.end().to_usize());
    occurrence.enclosing_range = if start_line == end_line {
        vec![start_line, start_character, end_character]
    } else {
        vec![start_line, start_character, end_line, end_character]
    };
    if start_line == end_line {
        occurrence.set_single_line_enclosing_range(SingleLineRange {
            line: start_line,
            start_character,
            end_character,
            ..Default::default()
        });
    } else {
        occurrence.set_multi_line_enclosing_range(MultiLineRange {
            start_line,
            start_character,
            end_line,
            end_character,
            ..Default::default()
        });
    }
    occurrence
}

fn position(source: &str, offset: usize) -> (i32, i32) {
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as i32;
    let character = before
        .rsplit_once('\n')
        .map_or(before.len(), |(_, tail)| tail.len()) as i32;
    (line, character)
}

fn source_slice(source: &str, range: TextRange) -> &str {
    &source[range.start().to_usize()..range.end().to_usize()]
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_have_typed_and_legacy_ranges() {
        let source = "def f():\n    pass\n";
        let occurrence = definition_occurrence(
            source,
            TextRange::new(4.into(), 5.into()),
            "symbol".into(),
            TextRange::new(0.into(), (source.len() as u32).into()),
        );

        assert_eq!(occurrence.range, [0, 4, 5]);
        assert_eq!(occurrence.enclosing_range, [0, 0, 2, 0]);
        assert!(occurrence.has_single_line_range());
        assert!(occurrence.has_multi_line_enclosing_range());
    }

    #[test]
    fn occurrence_positions_are_utf8_byte_offsets() {
        let occurrence = occurrence(
            "π = value\n",
            TextRange::new(5.into(), 10.into()),
            "symbol".into(),
            SymbolRole::ReadAccess as i32,
        );

        assert_eq!(occurrence.range, [0, 5, 10]);
        let typed = occurrence.single_line_range();
        assert_eq!(
            (typed.line, typed.start_character, typed.end_character),
            (0, 5, 10)
        );
    }
}
