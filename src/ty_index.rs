use std::{collections::HashMap, fs, path::PathBuf};

use ruff_db::{
    parsed::{ParsedModuleRef, parsed_module},
    source::source_text,
    system::{OsSystem, SystemPathBuf},
};
use ruff_python_ast::{
    AnyNodeRef, Expr, ExprContext, Identifier,
    visitor::source_order::{SourceOrderVisitor, TraversalSignal},
};
use ruff_text_size::{Ranged, TextRange};
use scip::types::SymbolRole;
use ty_ide::{HierarchicalSymbols, SymbolId, SymbolInfo, SymbolKind};
use ty_module_resolver::file_to_module;
use ty_project::{Db as _, ProjectDatabase, ProjectMetadata, SemanticDb as _};
use ty_python_core::{
    ProgramFile,
    definition::{DefinitionKind as TyDefinitionKind, docstring_from_body},
    place::ScopedPlaceId,
    scope::ScopeId,
    semantic_index,
};

use crate::scip_emit::{
    DefinitionKind, DescriptorKind, Edge, FileData, PackageIdentity, RelationshipEdge, SymbolData,
    SymbolDescriptor, global_symbol, is_named_member, local_symbol, member_symbol,
    parameter_symbol,
};

pub(crate) struct IndexData {
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<FileData>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) unresolved: usize,
    pub(crate) ambiguous: usize,
    pub(crate) external: usize,
    pub(crate) syntax_errors: usize,
    pub(crate) unsupported_syntax_errors: usize,
    pub(crate) samples: Vec<String>,
}

#[derive(Default)]
struct IdentifierRanges(Vec<TextRange>);

#[derive(Default)]
struct OccurrenceRoles(HashMap<TextRange, i32>);

impl OccurrenceRoles {
    fn add(&mut self, range: TextRange, role: SymbolRole) {
        *self.0.entry(range).or_default() |= role as i32;
    }

    fn add_context(&mut self, range: TextRange, context: ExprContext) {
        self.add(
            range,
            match context {
                ExprContext::Load | ExprContext::Invalid => SymbolRole::ReadAccess,
                ExprContext::Store | ExprContext::Del => SymbolRole::WriteAccess,
            },
        );
    }

    fn add_augmented_target(&mut self, target: &Expr) {
        match target {
            Expr::Name(name) => {
                self.add(name.range(), SymbolRole::ReadAccess);
                self.add(name.range(), SymbolRole::WriteAccess);
            }
            Expr::Attribute(attribute) => {
                self.add(attribute.attr.range(), SymbolRole::ReadAccess);
                self.add(attribute.attr.range(), SymbolRole::WriteAccess);
            }
            _ => {}
        }
    }
}

impl<'ast> SourceOrderVisitor<'ast> for OccurrenceRoles {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::ExprName(name) => self.add_context(name.range(), name.ctx),
            AnyNodeRef::ExprAttribute(attribute) => {
                self.add_context(attribute.attr.range(), attribute.ctx);
            }
            AnyNodeRef::StmtAugAssign(statement) => {
                self.add_augmented_target(&statement.target);
            }
            AnyNodeRef::Keyword(keyword) => {
                if let Some(name) = &keyword.arg {
                    self.add(name.range(), SymbolRole::ReadAccess);
                }
            }
            AnyNodeRef::PatternKeyword(keyword) => {
                self.add(keyword.attr.range(), SymbolRole::ReadAccess);
            }
            AnyNodeRef::StmtImportFrom(statement) => {
                if let Some(module) = &statement.module {
                    self.add(module.range(), SymbolRole::Import);
                }
            }
            AnyNodeRef::Alias(alias) => {
                self.add(alias.range(), SymbolRole::Import);
                self.add(alias.name.range(), SymbolRole::Import);
                if let Some(name) = &alias.asname {
                    self.add(name.range(), SymbolRole::Import);
                }
            }
            AnyNodeRef::ExceptHandlerExceptHandler(handler) => {
                if let Some(name) = &handler.name {
                    self.add(name.range(), SymbolRole::WriteAccess);
                }
            }
            AnyNodeRef::PatternMatchMapping(pattern) => {
                if let Some(name) = &pattern.rest {
                    self.add(name.range(), SymbolRole::WriteAccess);
                }
            }
            AnyNodeRef::PatternMatchStar(pattern) => {
                if let Some(name) = &pattern.name {
                    self.add(name.range(), SymbolRole::WriteAccess);
                }
            }
            AnyNodeRef::PatternMatchAs(pattern) => {
                if let Some(name) = &pattern.name {
                    self.add(name.range(), SymbolRole::WriteAccess);
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }
}

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
    callables: Vec<Option<SymbolData>>,
    globals: &'symbols mut HashMap<TextRange, SymbolData>,
}

#[derive(Clone)]
struct InstanceAttributeCandidate {
    attribute_range: TextRange,
    class: SymbolData,
    name: String,
}

struct InstanceAttributeCandidates<'symbols> {
    classes: Vec<(usize, Option<SymbolData>)>,
    callables: Vec<Option<SymbolData>>,
    globals: &'symbols HashMap<TextRange, SymbolData>,
    candidates: HashMap<TextRange, InstanceAttributeCandidate>,
}

impl<'ast> SourceOrderVisitor<'ast> for InstanceAttributeCandidates<'_> {
    fn enter_node(&mut self, node: AnyNodeRef<'ast>) -> TraversalSignal {
        match node {
            AnyNodeRef::StmtClassDef(class) => self.classes.push((
                self.callables.len(),
                self.globals.get(&class.name.range()).cloned(),
            )),
            AnyNodeRef::StmtFunctionDef(function) => {
                let class = self
                    .classes
                    .last()
                    .filter(|(callable_depth, _)| {
                        *callable_depth == self.callables.len()
                            && function.decorator_list.is_empty()
                    })
                    .and_then(|(_, class)| class.clone());
                self.callables.push(class);
            }
            AnyNodeRef::ExprLambda(_) => self.callables.push(None),
            AnyNodeRef::ExprAttribute(attribute) => {
                if let Some(Some(class)) = self.callables.last() {
                    self.candidates.insert(
                        attribute.range(),
                        InstanceAttributeCandidate {
                            attribute_range: attribute.attr.range(),
                            class: class.clone(),
                            name: attribute.attr.to_string(),
                        },
                    );
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }

    fn leave_node(&mut self, node: AnyNodeRef<'ast>) {
        match node {
            AnyNodeRef::StmtClassDef(_) => {
                self.classes.pop();
            }
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::ExprLambda(_) => {
                self.callables.pop();
            }
            _ => {}
        }
    }
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

pub(crate) fn index(
    discovery_root: PathBuf,
    sample_limit: usize,
    project_name: Option<String>,
    project_version: Option<String>,
) -> Result<IndexData, String> {
    let system_root = SystemPathBuf::from_path_buf(discovery_root)
        .map_err(|path| format!("project path is not UTF-8: {}", path.display()))?;
    let system = OsSystem::new(&system_root);
    let metadata =
        ProjectMetadata::discover(&system_root, &system).map_err(|error| error.to_string())?;
    let db = ProjectDatabase::fallible(metadata, system).map_err(|error| error.to_string())?;
    let root = db.project().root(&db).as_std_path().to_path_buf();
    let package = package_identity(&root, project_name, project_version)?;

    let mut files = db.project().files(&db).iter().collect::<Vec<_>>();
    files.sort_by_key(|file| file.path(&db).to_string());
    let file_indices = files
        .iter()
        .enumerate()
        .map(|(index, file)| (*file, index))
        .collect::<HashMap<_, _>>();
    let mut data = Vec::with_capacity(files.len());
    let mut syntax_errors = 0;
    let mut unsupported_syntax_errors = 0;
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
        let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);
        syntax_errors += parsed.errors().len();
        unsupported_syntax_errors += parsed.unsupported_syntax_errors().len();
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
        let mut module_symbol = global_symbol(
            &package,
            &module_descriptors,
            module_name,
            DefinitionKind::Module,
            TextRange::new(0.into(), (source.len() as u32).into()),
        );
        module_symbol.documentation = {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            docstring_from_body(module.suite())
                .map(|expression| expression.value.to_str().to_owned())
                .into_iter()
                .collect()
        };
        let mut globals = HashMap::from([(TextRange::default(), module_symbol)]);
        for (id, info) in hierarchy.iter() {
            collect_global_symbols(
                &hierarchy,
                id,
                info,
                &package,
                &module_descriptors,
                &mut globals,
            );
        }
        {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            ParameterSymbols {
                callables: Vec::new(),
                globals: &mut globals,
            }
            .visit_body(module.suite());
        }
        let occurrence_roles = {
            let module = parsed_module(&db, program_file.python_file(&db)).load(&db);
            let mut roles = OccurrenceRoles::default();
            roles.visit_body(module.suite());
            roles.0
        };
        let (locals, semantic_bindings, canonical_definition_ranges) =
            allocate_semantic_symbols(&db, program_file, &source, &mut globals);
        data.push(FileData {
            relative_path,
            source,
            globals,
            locals,
            semantic_bindings,
            canonical_definition_ranges,
            occurrence_roles,
            relationships: Vec::new(),
        });
    }

    let relationships = collect_relationships(&db, &files, &file_indices, &data);
    for (file, relationships) in data.iter_mut().zip(relationships) {
        file.relationships = relationships;
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
                .map(|target| {
                    let file = target.file();
                    let range = target.focus_range();
                    let range = file_indices
                        .get(&file)
                        .and_then(|index| data[*index].canonical_definition_ranges.get(&range))
                        .copied()
                        .unwrap_or(range);
                    (file, range)
                })
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

    Ok(IndexData {
        root,
        files: data,
        edges,
        unresolved,
        ambiguous,
        external,
        syntax_errors,
        unsupported_syntax_errors,
        samples,
    })
}

fn collect_relationships(
    db: &ProjectDatabase,
    files: &[ruff_db::files::File],
    file_indices: &HashMap<ruff_db::files::File, usize>,
    data: &[FileData],
) -> Vec<Vec<RelationshipEdge>> {
    let mut relationships = vec![Vec::new(); data.len()];

    for (source_file, file) in data.iter().enumerate() {
        let program_file = db.program_file(files[source_file]);
        let mut classes = file
            .globals
            .iter()
            .filter(|(_, symbol)| symbol.kind == DefinitionKind::Class)
            .collect::<Vec<_>>();
        classes.sort_by_key(|(range, symbol)| (symbol.symbol.as_str(), range.start(), range.end()));
        classes.dedup_by(|(_, left), (_, right)| left.symbol == right.symbol);
        for (source_range, source) in classes {
            for target in ty_ide::type_hierarchy_supertypes(db, program_file, source_range.start())
            {
                let Some(target) =
                    first_party_global(file_indices, data, target.file, target.selection_range)
                else {
                    continue;
                };
                if source.symbol != target.symbol {
                    relationships[source_file].push(RelationshipEdge {
                        source_symbol: source.symbol.clone(),
                        target_symbol: target.symbol.clone(),
                    });
                }
            }
        }
    }

    for relationships in &mut relationships {
        relationships.sort();
        relationships.dedup();
    }
    relationships
}

fn first_party_global<'a>(
    file_indices: &HashMap<ruff_db::files::File, usize>,
    data: &'a [FileData],
    target_file: ruff_db::files::File,
    target_range: TextRange,
) -> Option<&'a SymbolData> {
    let target_file = *file_indices.get(&target_file)?;
    let target_range = data[target_file]
        .canonical_definition_ranges
        .get(&target_range)
        .copied()
        .unwrap_or(target_range);
    data[target_file]
        .globals
        .get(&target_range)
        .filter(|symbol| symbol.kind == DefinitionKind::Class && !symbol.is_local())
}

fn package_identity(
    root: &std::path::Path,
    name_override: Option<String>,
    version_override: Option<String>,
) -> Result<PackageIdentity, String> {
    let path = root.join("pyproject.toml");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let document = contents
        .parse::<toml::Table>()
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    let project = document.get("project").and_then(toml::Value::as_table);
    let field = |name| {
        project
            .and_then(|project| project.get(name))
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(PackageIdentity {
        name: name_override.unwrap_or_else(|| field("name")),
        version: version_override.unwrap_or_else(|| field("version")),
    })
}

type AllocatedSemanticSymbols = (
    HashMap<TextRange, SymbolData>,
    HashMap<TextRange, Vec<usize>>,
    HashMap<TextRange, TextRange>,
);

fn allocate_semantic_symbols<'db>(
    db: &'db dyn ty_python_core::Db,
    program_file: ProgramFile<'db>,
    source: &str,
    globals: &mut HashMap<TextRange, SymbolData>,
) -> AllocatedSemanticSymbols {
    let index = semantic_index(db, program_file);
    let module = parsed_module(db, program_file.python_file(db)).load(db);
    let mut attribute_candidates = InstanceAttributeCandidates {
        classes: Vec::new(),
        callables: Vec::new(),
        globals,
        candidates: HashMap::new(),
    };
    attribute_candidates.visit_body(module.suite());
    let mut definitions = Vec::new();
    let mut instance_symbols = HashMap::new();
    let mut canonical_definition_ranges = HashMap::new();
    let mut definition_full_ranges = HashMap::new();
    for scope in index.scope_ids() {
        for (_, definition, _) in index
            .use_def_map(scope.file_scope_id(db))
            .definitions_with_usage()
        {
            if definition.kind(db).is_user_visible() {
                let raw_range = definition.focus_range(db, &module).range();
                let range = attribute_candidates
                    .candidates
                    .get(&raw_range)
                    .filter(|candidate| {
                        ty_python_core::place_table(db, definition.scope(db))
                            .member_id_by_instance_attribute_name(&candidate.name)
                            .is_some_and(|member| definition.place(db) == member.into())
                    })
                    .map_or(raw_range, |candidate| {
                        let mut symbol =
                            member_symbol(&candidate.class, candidate.name.clone(), raw_range);
                        if let Some((_, existing)) = globals
                            .iter()
                            .filter(|(_, existing)| {
                                is_named_member(&candidate.class, existing, &candidate.name)
                            })
                            .min_by_key(|(range, _)| (range.start(), range.end()))
                        {
                            symbol = existing.clone();
                        }
                        instance_symbols.insert(candidate.attribute_range, symbol);
                        canonical_definition_ranges.insert(raw_range, candidate.attribute_range);
                        definition_full_ranges.insert(candidate.attribute_range, raw_range);
                        candidate.attribute_range
                    });
                let kind = definition.kind(db);
                definitions.push((
                    (definition.scope(db), definition.place(db)),
                    range,
                    definition.docstring(db).into_iter().collect::<Vec<_>>(),
                    definition_signature(kind, &module, source),
                ));
            }
        }
    }
    definitions.sort_by_key(|(_, range, _, _)| (range.start(), range.end()));

    let mut group_ids = HashMap::<(ScopeId<'db>, ScopedPlaceId), usize>::new();
    let mut groups = Vec::<Vec<(TextRange, Vec<String>, Option<String>)>>::new();
    let mut semantic_bindings = HashMap::<TextRange, Vec<usize>>::new();
    for (key, range, documentation, signature) in definitions {
        let group = *group_ids.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if let Some((_, existing_documentation, existing_signature)) = groups[group]
            .iter_mut()
            .find(|(existing_range, _, _)| *existing_range == range)
        {
            if existing_documentation.is_empty() {
                *existing_documentation = documentation;
            }
            if existing_signature.is_none() {
                *existing_signature = signature;
            }
        } else {
            groups[group].push((range, documentation, signature));
        }
        let bindings = semantic_bindings.entry(range).or_default();
        if !bindings.contains(&group) {
            bindings.push(group);
        }
    }

    let mut locals = HashMap::new();
    let mut next_local = 0;
    for ranges in groups {
        let ranges = ranges
            .into_iter()
            .filter(|(range, _, _)| semantic_bindings[range].len() == 1)
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            continue;
        }
        let existing = ranges
            .iter()
            .find_map(|(range, _, _)| globals.get(range).or_else(|| instance_symbols.get(range)))
            .cloned();
        let symbol = existing.unwrap_or_else(|| {
            let display_name = ranges
                .iter()
                .map(|(range, _, _)| source_slice(source, *range))
                .min_by_key(|name| (name.len(), *name))
                .expect("semantic definition group is not empty")
                .to_owned();
            let symbol = local_symbol(next_local, display_name, ranges[0].0);
            next_local += 1;
            symbol
        });
        for (range, documentation, signature) in ranges {
            let full_range = definition_full_ranges.get(&range).copied().unwrap_or(range);
            let symbol = SymbolData {
                full_range,
                documentation,
                signature,
                ..symbol.clone()
            };
            if symbol.is_local() {
                locals.insert(range, symbol);
            } else {
                globals
                    .entry(range)
                    .and_modify(|existing| {
                        if existing.documentation.is_empty() {
                            existing.documentation.clone_from(&symbol.documentation);
                        }
                        if existing.signature.is_none() {
                            existing.signature.clone_from(&symbol.signature);
                        }
                    })
                    .or_insert(symbol);
            }
        }
    }
    (locals, semantic_bindings, canonical_definition_ranges)
}

fn definition_signature(
    kind: &TyDefinitionKind<'_>,
    module: &ParsedModuleRef,
    source: &str,
) -> Option<String> {
    match kind {
        TyDefinitionKind::Function(definition) => {
            let function = definition.node(module);
            let end = function
                .returns
                .as_deref()
                .map_or_else(|| function.parameters.end(), Ranged::end);
            let header = source_slice(source, TextRange::new(function.name.start(), end));
            Some(format!(
                "{} {header}",
                if function.is_async {
                    "async def"
                } else {
                    "def"
                }
            ))
        }
        TyDefinitionKind::Class(definition) => {
            let class = definition.node(module);
            let end = class.arguments.as_deref().map_or_else(
                || {
                    class
                        .type_params
                        .as_deref()
                        .map_or_else(|| class.name.end(), Ranged::end)
                },
                Ranged::end,
            );
            Some(format!(
                "class {}",
                source_slice(source, TextRange::new(class.name.start(), end))
            ))
        }
        _ => None,
    }
}

fn collect_global_symbols(
    hierarchy: &HierarchicalSymbols,
    id: SymbolId,
    info: SymbolInfo<'_>,
    package: &PackageIdentity,
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
            package,
            &descriptors,
            info.name.to_string(),
            definition_kind,
            info.full_range,
        ),
    );
    for (child_id, child) in hierarchy.children(id) {
        collect_global_symbols(hierarchy, child_id, child, package, &descriptors, output);
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
        SymbolKind::Import => (DescriptorKind::Term, DefinitionKind::Import),
    }
}

fn source_slice(source: &str, range: TextRange) -> &str {
    &source[range.start().to_usize()..range.end().to_usize()]
}
