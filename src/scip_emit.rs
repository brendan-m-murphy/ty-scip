use std::{collections::HashMap, env, fs, path::Path};

use ruff_source_file::{LineIndex, PositionEncoding as RuffPositionEncoding};
use ruff_text_size::TextRange;
use scip::{
    symbol::{format_symbol, parse_symbol},
    types::{
        Descriptor, Document, Index, Metadata, MultiLineRange, Occurrence, Package,
        PositionEncoding, ProtocolVersion, Signature, SingleLineRange, Symbol, SymbolInformation,
        SymbolRole, TextEncoding, ToolInfo, descriptor, symbol_information,
    },
    write_message_to_file,
};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DefinitionKind {
    Module,
    Import,
    Class,
    Method,
    Function,
    Constructor,
    Parameter,
    TypeParameter,
    Variable,
    Constant,
    Property,
    Field,
}

#[derive(Clone, Copy)]
pub(crate) enum DescriptorKind {
    Namespace,
    Type,
    Method,
    Parameter,
    TypeParameter,
    Term,
}

#[derive(Clone)]
pub(crate) struct SymbolDescriptor {
    pub(crate) name: String,
    pub(crate) kind: DescriptorKind,
}

#[derive(Clone, Default)]
pub(crate) struct PackageIdentity {
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(Clone)]
pub(crate) struct SymbolData {
    pub(crate) symbol: String,
    pub(crate) display_name: String,
    pub(crate) kind: DefinitionKind,
    pub(crate) full_range: TextRange,
    pub(crate) documentation: Vec<String>,
    pub(crate) signature: Option<String>,
}

impl SymbolData {
    pub(crate) fn is_local(&self) -> bool {
        self.symbol.starts_with("local ")
    }
}

pub(crate) struct FileData {
    pub(crate) relative_path: String,
    pub(crate) source: String,
    pub(crate) globals: HashMap<TextRange, SymbolData>,
    pub(crate) locals: HashMap<TextRange, SymbolData>,
    pub(crate) semantic_bindings: HashMap<TextRange, Vec<usize>>,
    pub(crate) canonical_definition_ranges: HashMap<TextRange, TextRange>,
    pub(crate) occurrence_roles: HashMap<TextRange, i32>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Edge {
    pub(crate) source_file: usize,
    pub(crate) source_range: TextRange,
    pub(crate) target_file: usize,
    pub(crate) target_range: TextRange,
}

pub(crate) fn global_symbol(
    package: &PackageIdentity,
    descriptors: &[SymbolDescriptor],
    display_name: String,
    kind: DefinitionKind,
    full_range: TextRange,
) -> SymbolData {
    SymbolData {
        symbol: format_symbol(Symbol {
            scheme: "ty-scip".into(),
            package: Some(Package {
                manager: "python".into(),
                name: package.name.clone(),
                version: package.version.clone(),
                ..Default::default()
            })
            .into(),
            descriptors: descriptors.iter().map(scip_descriptor).collect(),
            ..Default::default()
        }),
        display_name,
        kind,
        full_range,
        documentation: Vec::new(),
        signature: None,
    }
}

pub(crate) fn parameter_symbol(
    parent: &SymbolData,
    name: String,
    full_range: TextRange,
) -> SymbolData {
    let mut symbol = parse_symbol(&parent.symbol).expect("ty-scip formatted symbol");
    symbol.descriptors.push(Descriptor {
        name: name.clone(),
        suffix: descriptor::Suffix::Parameter.into(),
        ..Default::default()
    });
    SymbolData {
        symbol: format_symbol(symbol),
        display_name: name,
        kind: DefinitionKind::Parameter,
        full_range,
        documentation: Vec::new(),
        signature: None,
    }
}

pub(crate) fn member_symbol(
    parent: &SymbolData,
    name: String,
    full_range: TextRange,
) -> SymbolData {
    let mut symbol = parse_symbol(&parent.symbol).expect("ty-scip formatted symbol");
    symbol.descriptors.push(Descriptor {
        name: name.clone(),
        suffix: descriptor::Suffix::Term.into(),
        ..Default::default()
    });
    SymbolData {
        symbol: format_symbol(symbol),
        display_name: name,
        kind: DefinitionKind::Field,
        full_range,
        documentation: Vec::new(),
        signature: None,
    }
}

pub(crate) fn is_named_member(parent: &SymbolData, member: &SymbolData, name: &str) -> bool {
    let Ok(parent) = parse_symbol(&parent.symbol) else {
        return false;
    };
    let Ok(member) = parse_symbol(&member.symbol) else {
        return false;
    };
    member.scheme == parent.scheme
        && member.package == parent.package
        && member.descriptors.len() == parent.descriptors.len() + 1
        && member.descriptors.starts_with(&parent.descriptors)
        && member
            .descriptors
            .last()
            .is_some_and(|item| item.name == name)
}

pub(crate) fn local_symbol(
    index: usize,
    display_name: String,
    full_range: TextRange,
) -> SymbolData {
    SymbolData {
        symbol: format_symbol(Symbol::new_local(index)),
        display_name,
        kind: DefinitionKind::Variable,
        full_range,
        documentation: Vec::new(),
        signature: None,
    }
}

pub(crate) fn write_index(
    root: &Path,
    output: &Path,
    data: &[FileData],
    edges: &[Edge],
) -> Result<(), String> {
    let line_indices = data
        .iter()
        .map(|file| LineIndex::from_source_text(&file.source))
        .collect::<Vec<_>>();
    let mut documents = data
        .iter()
        .map(|file| Document {
            language: "python".into(),
            relative_path: file.relative_path.clone(),
            symbols: symbol_information(&file.globals, &file.locals),
            position_encoding: PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    for ((document, file), line_index) in documents.iter_mut().zip(data).zip(&line_indices) {
        let mut definitions = file.globals.iter().chain(&file.locals).collect::<Vec<_>>();
        definitions
            .sort_by_key(|(range, symbol)| (range.start(), range.end(), symbol.symbol.as_str()));
        document
            .occurrences
            .extend(definitions.into_iter().map(|(range, symbol)| {
                definition_occurrence(
                    &file.source,
                    line_index,
                    *range,
                    symbol.symbol.clone(),
                    symbol.full_range,
                    file.occurrence_roles
                        .get(range)
                        .copied()
                        .unwrap_or_default()
                        | if matches!(symbol.kind, DefinitionKind::Import) {
                            SymbolRole::Import as i32
                        } else {
                            0
                        },
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
        if edge.source_file != edge.target_file && symbol.is_local() {
            continue;
        }
        documents[edge.source_file].occurrences.push(occurrence(
            &data[edge.source_file].source,
            &line_indices[edge.source_file],
            edge.source_range,
            symbol.symbol.clone(),
            data[edge.source_file]
                .occurrence_roles
                .get(&edge.source_range)
                .copied()
                .unwrap_or(SymbolRole::ReadAccess as i32),
        ));
    }
    for document in &mut documents {
        document
            .symbols
            .sort_by(|left, right| left.symbol.cmp(&right.symbol));
        document
            .symbols
            .dedup_by(|left, right| left.symbol == right.symbol);
        merge_occurrences(&mut document.occurrences);
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

fn scip_descriptor(descriptor: &SymbolDescriptor) -> Descriptor {
    Descriptor {
        name: descriptor.name.clone(),
        suffix: match descriptor.kind {
            DescriptorKind::Namespace => descriptor::Suffix::Namespace,
            DescriptorKind::Type => descriptor::Suffix::Type,
            DescriptorKind::Method => descriptor::Suffix::Method,
            DescriptorKind::Parameter => descriptor::Suffix::Parameter,
            DescriptorKind::TypeParameter => descriptor::Suffix::TypeParameter,
            DescriptorKind::Term => descriptor::Suffix::Term,
        }
        .into(),
        ..Default::default()
    }
}

fn symbol_kind(kind: DefinitionKind) -> symbol_information::Kind {
    match kind {
        DefinitionKind::Module => symbol_information::Kind::Module,
        DefinitionKind::Import => symbol_information::Kind::Module,
        DefinitionKind::Class => symbol_information::Kind::Class,
        DefinitionKind::Method => symbol_information::Kind::Method,
        DefinitionKind::Function => symbol_information::Kind::Function,
        DefinitionKind::Constructor => symbol_information::Kind::Constructor,
        DefinitionKind::Parameter => symbol_information::Kind::Parameter,
        DefinitionKind::TypeParameter => symbol_information::Kind::TypeParameter,
        DefinitionKind::Variable => symbol_information::Kind::Variable,
        DefinitionKind::Constant => symbol_information::Kind::Constant,
        DefinitionKind::Property => symbol_information::Kind::Property,
        DefinitionKind::Field => symbol_information::Kind::Field,
    }
}

fn symbol_information(
    globals: &HashMap<TextRange, SymbolData>,
    locals: &HashMap<TextRange, SymbolData>,
) -> Vec<SymbolInformation> {
    let mut symbols = globals.iter().chain(locals).collect::<Vec<_>>();
    symbols.sort_by(|(left_range, left), (right_range, right)| {
        left.symbol
            .cmp(&right.symbol)
            .then_with(|| {
                (left_range.start(), left_range.end())
                    .cmp(&(right_range.start(), right_range.end()))
            })
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.documentation.cmp(&right.documentation))
            .then_with(|| left.signature.cmp(&right.signature))
    });
    let mut information: Vec<SymbolInformation> = Vec::new();
    for (_, symbol) in symbols {
        if let Some(previous) = information.last_mut()
            && previous.symbol == symbol.symbol
        {
            if previous.documentation.is_empty() && !symbol.documentation.is_empty() {
                previous.documentation.clone_from(&symbol.documentation);
            }
            if previous.signature_documentation.is_none()
                && let Some(signature) = &symbol.signature
            {
                previous.signature_documentation = Some(python_signature(signature)).into();
            }
            continue;
        }
        information.push(SymbolInformation {
            symbol: symbol.symbol.clone(),
            documentation: symbol.documentation.clone(),
            kind: symbol_kind(symbol.kind).into(),
            display_name: symbol.display_name.clone(),
            signature_documentation: symbol.signature.as_deref().map(python_signature).into(),
            ..Default::default()
        });
    }
    information
}

fn python_signature(text: &str) -> Signature {
    Signature {
        language: "python".into(),
        text: text.into(),
        ..Default::default()
    }
}

fn occurrence(
    source: &str,
    line_index: &LineIndex,
    range: TextRange,
    symbol: String,
    roles: i32,
) -> Occurrence {
    let (line, start_character) = position(line_index, source, range.start());
    let (end_line, end_character) = position(line_index, source, range.end());
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
    line_index: &LineIndex,
    range: TextRange,
    symbol: String,
    enclosing_range: TextRange,
    roles: i32,
) -> Occurrence {
    let mut occurrence = occurrence(
        source,
        line_index,
        range,
        symbol,
        SymbolRole::Definition as i32 | roles,
    );
    let (start_line, start_character) = position(line_index, source, enclosing_range.start());
    let (end_line, end_character) = position(line_index, source, enclosing_range.end());
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

fn merge_occurrences(occurrences: &mut Vec<Occurrence>) {
    occurrences.sort_by(|left, right| {
        left.range
            .cmp(&right.range)
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    let mut merged: Vec<Occurrence> = Vec::with_capacity(occurrences.len());
    for occurrence in occurrences.drain(..) {
        if let Some(previous) = merged.last_mut()
            && previous.range == occurrence.range
            && previous.symbol == occurrence.symbol
        {
            previous.symbol_roles |= occurrence.symbol_roles;
            if previous.enclosing_range.is_empty() {
                previous.enclosing_range = occurrence.enclosing_range;
            }
            if previous.typed_enclosing_range.is_none() {
                previous.typed_enclosing_range = occurrence.typed_enclosing_range;
            }
        } else {
            merged.push(occurrence);
        }
    }
    *occurrences = merged;
}

fn position(line_index: &LineIndex, source: &str, offset: ruff_text_size::TextSize) -> (i32, i32) {
    let location = line_index.source_location(offset, source, RuffPositionEncoding::Utf8);
    (
        location.line.to_zero_indexed() as i32,
        location.character_offset.to_zero_indexed() as i32,
    )
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
        let line_index = LineIndex::from_source_text(source);
        let occurrence = definition_occurrence(
            source,
            &line_index,
            TextRange::new(4.into(), 5.into()),
            "symbol".into(),
            TextRange::new(0.into(), (source.len() as u32).into()),
            0,
        );

        assert_eq!(occurrence.range, [0, 4, 5]);
        assert_eq!(occurrence.enclosing_range, [0, 0, 2, 0]);
        assert!(occurrence.has_single_line_range());
        assert!(occurrence.has_multi_line_enclosing_range());
    }

    #[test]
    fn occurrence_positions_are_utf8_byte_offsets() {
        let source = "π = value\n";
        let line_index = LineIndex::from_source_text(source);
        let occurrence = occurrence(
            source,
            &line_index,
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

    #[test]
    fn occurrence_positions_follow_universal_newlines() {
        for source in ["def f():\r\n    π = value\r\n", "def f():\r    π = value\r"] {
            let line_index = LineIndex::from_source_text(source);
            let start = source.find("value").unwrap() as u32;
            let occurrence = definition_occurrence(
                source,
                &line_index,
                TextRange::new(start.into(), (start + 5).into()),
                "symbol".into(),
                TextRange::new(0.into(), (source.len() as u32).into()),
                0,
            );

            assert_eq!(occurrence.range, [1, 9, 14]);
            assert_eq!(occurrence.enclosing_range, [0, 0, 2, 0]);
            assert!(occurrence.has_single_line_range());
            assert!(occurrence.has_multi_line_enclosing_range());
        }
    }

    #[test]
    fn symbol_information_uses_the_earliest_definition() {
        let symbol = |display_name: &str, start: u32| SymbolData {
            symbol: "local 0".into(),
            display_name: display_name.into(),
            kind: DefinitionKind::Variable,
            full_range: TextRange::new(start.into(), (start + 1).into()),
            documentation: (start == 20)
                .then(|| "later documentation".into())
                .into_iter()
                .collect(),
            signature: (start == 20).then(|| "def later()".into()),
        };
        let globals = HashMap::new();
        let locals = HashMap::from([
            (TextRange::new(20.into(), 21.into()), symbol("later", 20)),
            (TextRange::new(10.into(), 11.into()), symbol("earlier", 10)),
        ]);

        let information = symbol_information(&globals, &locals);

        assert_eq!(information.len(), 1);
        assert_eq!(information[0].display_name, "earlier");
        assert_eq!(information[0].documentation, ["later documentation"]);
        assert_eq!(
            information[0]
                .signature_documentation
                .as_ref()
                .unwrap()
                .text,
            "def later()"
        );
    }

    #[test]
    fn merging_occurrences_preserves_definition_metadata() {
        let source = "value = 1\n";
        let line_index = LineIndex::from_source_text(source);
        let range = TextRange::new(0.into(), 5.into());
        let mut occurrences = vec![
            occurrence(
                source,
                &line_index,
                range,
                "local 0".into(),
                SymbolRole::ReadAccess as i32,
            ),
            definition_occurrence(
                source,
                &line_index,
                range,
                "local 0".into(),
                TextRange::new(0.into(), (source.len() as u32).into()),
                0,
            ),
        ];

        merge_occurrences(&mut occurrences);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].symbol_roles, 9);
        assert_eq!(occurrences[0].enclosing_range, [0, 0, 1, 0]);
        assert!(occurrences[0].has_multi_line_enclosing_range());
    }
}
