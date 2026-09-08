use std::{collections::HashMap, path::PathBuf};

use ruff_db::{
    parsed::parsed_module,
    source::source_text,
    system::{OsSystem, SystemPathBuf},
};
use ruff_python_ast::{
    AnyNodeRef, Identifier,
    visitor::source_order::{SourceOrderVisitor, TraversalSignal},
};
use ruff_text_size::{Ranged, TextRange};
use ty_ide::{HierarchicalSymbols, SymbolId, SymbolInfo, SymbolKind};
use ty_module_resolver::file_to_module;
use ty_project::{Db as _, ProjectDatabase, ProjectMetadata, SemanticDb as _};
use ty_python_core::{ProgramFile, place::ScopedPlaceId, scope::ScopeId, semantic_index};

use crate::scip_emit::{
    DefinitionKind, DescriptorKind, Edge, FileData, SymbolData, SymbolDescriptor, global_symbol,
    local_symbol, parameter_symbol,
};

pub(crate) struct IndexData {
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<FileData>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) unresolved: usize,
    pub(crate) ambiguous: usize,
    pub(crate) external: usize,
    pub(crate) samples: Vec<String>,
}

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

struct DefinitionBindings<'db, 'output> {
    db: &'db dyn ty_python_core::Db,
    program_file: ProgramFile<'db>,
    groups: HashMap<(ScopeId<'db>, ScopedPlaceId), usize>,
    ranges: &'output mut HashMap<TextRange, Vec<usize>>,
}

impl<'ast> SourceOrderVisitor<'ast> for DefinitionBindings<'_, '_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        if let Some(definitions) = semantic_index(self.db, self.program_file).try_definitions(node)
        {
            for definition in definitions {
                let module = parsed_module(self.db, definition.python_file(self.db)).load(self.db);
                let range = definition.focus_range(self.db, &module).range();
                let key = (definition.scope(self.db), definition.place(self.db));
                let next = self.groups.len();
                let group = *self.groups.entry(key).or_insert(next);
                let groups = self.ranges.entry(range).or_default();
                if !groups.contains(&group) {
                    groups.push(group);
                }
            }
        }
        TraversalSignal::Traverse
    }
}

struct ParameterSymbols<'symbols> {
    callables: Vec<Option<SymbolData>>,
    globals: &'symbols mut HashMap<TextRange, SymbolData>,
}

impl<'ast> SourceOrderVisitor<'ast> for ParameterSymbols<'_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::StmtFunctionDef(function) => self
                .callables
                .push(self.globals.get(&function.name.range()).cloned()),
            AnyNodeRef::ExprLambda(_) => self.callables.push(None),
            AnyNodeRef::Parameter(parameter) => {
                if let Some(Some(parent)) = self.callables.last() {
                    self.globals.insert(
                        parameter.name.range(),
                        parameter_symbol(parent, parameter.name.to_string(), parameter.range()),
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

pub(crate) fn index(discovery_root: PathBuf, sample_limit: usize) -> Result<IndexData, String> {
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
    for &file in &files {
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
            .map(|name| SymbolDescriptor {
                name: name.to_owned(),
                kind: DescriptorKind::Namespace,
            })
            .collect::<Vec<_>>();
        let module_name = module_descriptors
            .last()
            .map(|descriptor| descriptor.name.clone())
            .unwrap_or_default();
        let module_symbol = global_symbol(
            &module_descriptors,
            module_name,
            DefinitionKind::Module,
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
        let mut semantic_bindings = HashMap::new();
        {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            DefinitionBindings {
                db: &db,
                program_file,
                groups: HashMap::new(),
                ranges: &mut semantic_bindings,
            }
            .visit_body(module.suite());
        }
        data.push(FileData {
            relative_path,
            source,
            globals,
            locals: HashMap::new(),
            semantic_bindings,
        });
    }

    let mut edges = Vec::new();
    let mut unresolved = 0;
    let mut ambiguous = 0;
    let mut external = 0;
    let mut unresolved_samples = 0;
    let mut ambiguous_samples = 0;
    let mut samples = Vec::new();
    for (source_index, file_data) in data.iter().enumerate() {
        let program_file = db.program_file(files[source_index]);
        let ranges = {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            let mut visitor = IdentifierRanges::default();
            visitor.visit_body(module.suite());
            visitor.0
        };

        for range in ranges {
            let mut targets = ty_ide::goto_declaration(&db, program_file, range.start())
                .into_iter()
                .flat_map(|result| result.value)
                .map(|target| (target.file(), target.focus_range()))
                .collect::<Vec<_>>();
            targets.sort_unstable_by_key(|(file, range)| {
                (file.path(&db).to_string(), range.start(), range.end())
            });
            targets.dedup();
            if targets.len() > 1 {
                let normalized = targets
                    .iter()
                    .map(|(file, range)| {
                        let file = *file_indices.get(file)?;
                        Some((file, *range, data[file].globals.get(range)?.symbol.as_str()))
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(normalized) = normalized
                    && let Some((target_file, target_range, symbol)) = normalized.first()
                    && normalized
                        .iter()
                        .all(|(_, _, candidate)| candidate == symbol)
                {
                    if !normalized
                        .iter()
                        .any(|(candidate_file, candidate_range, _)| {
                            *candidate_file == source_index && *candidate_range == range
                        })
                    {
                        edges.push(Edge {
                            source_file: source_index,
                            source_range: range,
                            target_file: *target_file,
                            target_range: *target_range,
                        });
                    }
                    continue;
                }
                let normalized = targets
                    .iter()
                    .map(|(file, range)| {
                        let file = *file_indices.get(file)?;
                        let groups = data[file].semantic_bindings.get(range)?;
                        (groups.len() == 1).then_some((file, groups[0]))
                    })
                    .collect::<Option<Vec<_>>>();
                let mut global_symbols = targets.iter().filter_map(|(file, range)| {
                    let file = *file_indices.get(file)?;
                    Some(data[file].globals.get(range)?.symbol.as_str())
                });
                let first_global = global_symbols.next();
                let global_symbols_agree =
                    global_symbols.all(|candidate| Some(candidate) == first_global);
                if let Some(normalized) = normalized
                    && let Some(binding) = normalized.first()
                    && normalized.iter().all(|candidate| candidate == binding)
                    && global_symbols_agree
                {
                    if !targets.iter().any(|(candidate_file, candidate_range)| {
                        file_indices.get(candidate_file) == Some(&source_index)
                            && *candidate_range == range
                    }) {
                        let (target_file, target_range) = targets
                            .iter()
                            .find(|(file, range)| {
                                file_indices
                                    .get(file)
                                    .is_some_and(|file| data[*file].globals.contains_key(range))
                            })
                            .unwrap_or(&targets[0]);
                        edges.push(Edge {
                            source_file: source_index,
                            source_range: range,
                            target_file: file_indices[target_file],
                            target_range: *target_range,
                        });
                    }
                    continue;
                }
                if targets
                    .iter()
                    .all(|(file, _)| !file_indices.contains_key(file))
                {
                    external += 1;
                    continue;
                }
            }
            match targets.as_slice() {
                [] => {
                    unresolved += 1;
                    if unresolved_samples < sample_limit {
                        samples.push(format!(
                            "unresolved {}:{range:?} {:?}",
                            file_data.relative_path,
                            source_slice(&file_data.source, range)
                        ));
                        unresolved_samples += 1;
                    }
                }
                [(target_file, target_range)] => {
                    if let Some(target_file) = file_indices.get(target_file) {
                        edges.push(Edge {
                            source_file: source_index,
                            source_range: range,
                            target_file: *target_file,
                            target_range: *target_range,
                        });
                    } else {
                        external += 1;
                    }
                }
                _ => {
                    ambiguous += 1;
                    if ambiguous_samples < sample_limit {
                        let candidates = targets
                            .iter()
                            .map(|(file, range)| {
                                if let Some(index) = file_indices.get(file) {
                                    let symbol = data[*index]
                                        .globals
                                        .get(range)
                                        .map_or("?", |symbol| symbol.symbol.as_str());
                                    format!("{}:{range:?}={symbol}", data[*index].relative_path)
                                } else {
                                    format!("{}:{range:?}=external", file.path(&db))
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        samples.push(format!(
                            "ambiguous {}:{range:?} {:?} -> {candidates}",
                            file_data.relative_path,
                            source_slice(&file_data.source, range)
                        ));
                        ambiguous_samples += 1;
                    }
                }
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

    allocate_local_symbols(&mut data, &edges);

    Ok(IndexData {
        root,
        files: data,
        edges,
        unresolved,
        ambiguous,
        external,
        samples,
    })
}

fn allocate_local_symbols(data: &mut [FileData], edges: &[Edge]) {
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
        let mut allocated_groups = Vec::new();
        for range in ranges {
            let group = file_data
                .semantic_bindings
                .get(&range)
                .filter(|groups| groups.len() == 1)
                .map(|groups| groups[0]);
            if group.is_some_and(|group| allocated_groups.contains(&group)) {
                continue;
            }
            if let Some(group) = group {
                allocated_groups.push(group);
            }
            let mut definition_ranges = group.map_or_else(
                || vec![range],
                |group| {
                    file_data
                        .semantic_bindings
                        .iter()
                        .filter_map(|(range, groups)| {
                            (groups.len() == 1 && groups[0] == group).then_some(*range)
                        })
                        .collect()
                },
            );
            definition_ranges.sort_unstable_by_key(|range| (range.start(), range.end()));
            let display_name = definition_ranges
                .iter()
                .map(|range| source_slice(&file_data.source, *range))
                .min_by_key(|name| (name.len(), *name))
                .expect("local definition group is not empty")
                .to_owned();
            let symbol = local_symbol(file_data.locals.len(), display_name, range);
            for definition_range in definition_ranges {
                file_data.locals.insert(
                    definition_range,
                    SymbolData {
                        full_range: definition_range,
                        ..symbol.clone()
                    },
                );
            }
        }
    }
}

fn collect_global_symbols(
    hierarchy: &HierarchicalSymbols,
    id: SymbolId,
    info: SymbolInfo<'_>,
    parents: &[SymbolDescriptor],
    output: &mut HashMap<TextRange, SymbolData>,
) {
    let (descriptor_kind, definition_kind) = symbol_kinds(info.kind);
    let mut descriptors = parents.to_vec();
    descriptors.push(SymbolDescriptor {
        name: info.name.to_string(),
        kind: descriptor_kind,
    });
    output.insert(
        info.name_range,
        global_symbol(
            &descriptors,
            info.name.to_string(),
            definition_kind,
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

fn symbol_kinds(kind: SymbolKind) -> (DescriptorKind, DefinitionKind) {
    match kind {
        SymbolKind::Module => (DescriptorKind::Namespace, DefinitionKind::Module),
        SymbolKind::Class => (DescriptorKind::Type, DefinitionKind::Class),
        SymbolKind::Method => (DescriptorKind::Method, DefinitionKind::Method),
        SymbolKind::Function => (DescriptorKind::Method, DefinitionKind::Function),
        SymbolKind::Constructor => (DescriptorKind::Method, DefinitionKind::Constructor),
        SymbolKind::Parameter => (DescriptorKind::Parameter, DefinitionKind::Parameter),
        SymbolKind::TypeParameter => (DescriptorKind::TypeParameter, DefinitionKind::TypeParameter),
        SymbolKind::Variable => (DescriptorKind::Term, DefinitionKind::Variable),
        SymbolKind::Constant => (DescriptorKind::Term, DefinitionKind::Constant),
        SymbolKind::Property => (DescriptorKind::Term, DefinitionKind::Property),
        SymbolKind::Field => (DescriptorKind::Term, DefinitionKind::Field),
        SymbolKind::Import => (DescriptorKind::Term, DefinitionKind::Module),
    }
}

fn source_slice(source: &str, range: TextRange) -> &str {
    &source[range.start().to_usize()..range.end().to_usize()]
}
