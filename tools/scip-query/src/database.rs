//! Lossless normalized SQLite cache and high-signal reference projections.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::path::Path;

use protobuf::Message;
use rusqlite::{Connection, OpenFlags, params};
use scip::types::{SymbolRole, symbol_information};
use serde::Serialize;

use super::{QueryIndex, RefDirection, SourceRange, SymbolId};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const SCHEMA: &str = r#"
CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE documents (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  language TEXT NOT NULL,
  position_encoding INTEGER NOT NULL,
  text TEXT NOT NULL
);
CREATE TABLE symbols (
  id INTEGER PRIMARY KEY,
  identity_document TEXT NOT NULL,
  scip_symbol TEXT NOT NULL,
  canonical TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  qualified_name TEXT NOT NULL,
  kinds_json TEXT NOT NULL,
  UNIQUE(identity_document, scip_symbol)
);
CREATE TABLE symbol_information (
  id INTEGER PRIMARY KEY,
  symbol_id INTEGER NOT NULL REFERENCES symbols(id),
  document TEXT,
  source_index INTEGER NOT NULL,
  external INTEGER NOT NULL,
  documentation_json TEXT NOT NULL,
  kind INTEGER NOT NULL,
  display_name TEXT NOT NULL,
  enclosing_symbol TEXT NOT NULL,
  signature_language TEXT,
  signature_text TEXT,
  raw_protobuf BLOB NOT NULL
);
CREATE TABLE occurrences (
  id INTEGER PRIMARY KEY,
  document_id INTEGER NOT NULL REFERENCES documents(id),
  occurrence_index INTEGER NOT NULL,
  symbol_id INTEGER REFERENCES symbols(id),
  owner_symbol_id INTEGER REFERENCES symbols(id),
  start_line INTEGER,
  start_character INTEGER,
  end_line INTEGER,
  end_character INTEGER,
  range_source TEXT NOT NULL,
  legacy_range_json TEXT NOT NULL,
  enclosing_start_line INTEGER,
  enclosing_start_character INTEGER,
  enclosing_end_line INTEGER,
  enclosing_end_character INTEGER,
  enclosing_range_source TEXT NOT NULL,
  legacy_enclosing_range_json TEXT NOT NULL,
  roles INTEGER NOT NULL,
  role_names_json TEXT NOT NULL,
  position_encoding INTEGER NOT NULL,
  syntax_kind INTEGER NOT NULL,
  override_documentation_json TEXT NOT NULL,
  diagnostic_count INTEGER NOT NULL,
  provenance TEXT NOT NULL,
  raw_protobuf BLOB NOT NULL,
  UNIQUE(document_id, occurrence_index)
);
CREATE TABLE relationships (
  id INTEGER PRIMARY KEY,
  source_symbol_id INTEGER NOT NULL REFERENCES symbols(id),
  target_symbol_id INTEGER NOT NULL REFERENCES symbols(id),
  document TEXT,
  symbol_information_index INTEGER NOT NULL,
  relationship_index INTEGER NOT NULL,
  is_reference INTEGER NOT NULL,
  is_implementation INTEGER NOT NULL,
  is_type_definition INTEGER NOT NULL,
  is_definition INTEGER NOT NULL,
  provenance TEXT NOT NULL,
  raw_protobuf BLOB NOT NULL
);
CREATE INDEX occurrences_symbol ON occurrences(symbol_id);
CREATE INDEX occurrences_owner ON occurrences(owner_symbol_id);
CREATE INDEX occurrences_document ON occurrences(document_id, start_line);
CREATE INDEX symbol_information_symbol ON symbol_information(symbol_id);
CREATE INDEX symbols_display ON symbols(display_name);
CREATE INDEX symbols_qualified ON symbols(qualified_name);
CREATE INDEX relationships_source ON relationships(source_symbol_id);
CREATE INDEX relationships_target ON relationships(target_symbol_id);
"#;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DatabaseStats {
    pub documents: usize,
    pub symbols: usize,
    pub symbol_information: usize,
    pub occurrences: usize,
    pub definitions: usize,
    pub relationships: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqlReferenceSummary {
    pub source: String,
    pub target: String,
    pub document: Option<String>,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub roles: Vec<String>,
    pub provenance: String,
    pub occurrences: usize,
    pub low_signal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<SqlRelationshipFlags>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SqlRelationshipFlags {
    pub is_reference: bool,
    pub is_implementation: bool,
    pub is_type_definition: bool,
    pub is_definition: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqlReferencePage {
    pub total: usize,
    pub offset: usize,
    pub returned: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
    pub items: Vec<SqlReferenceSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqlSymbolCandidate {
    pub canonical: String,
    pub qualified_name: String,
    pub display_name: String,
    pub kinds: Vec<i32>,
    pub definition: Option<String>,
}

#[derive(Debug)]
pub enum SqlSelectionError {
    NotFound {
        selector: String,
    },
    Ambiguous {
        selector: String,
        candidates: Vec<SqlSymbolCandidate>,
    },
}

impl fmt::Display for SqlSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { selector } => write!(formatter, "symbol not found: {selector}"),
            Self::Ambiguous {
                selector,
                candidates,
            } => write!(
                formatter,
                "ambiguous symbol {selector}: {}",
                candidates
                    .iter()
                    .map(|candidate| candidate.qualified_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl Error for SqlSelectionError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqlTestSummary {
    pub document: String,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub target: String,
    pub match_kind: String,
    pub roles: Vec<String>,
    pub occurrences: usize,
    pub depth: usize,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SqlTestPage {
    pub total: usize,
    pub offset: usize,
    pub returned: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
    pub items: Vec<SqlTestSummary>,
}

impl QueryIndex {
    /// Materialize all normalized semantic facts without replacing the SCIP authority.
    pub fn write_database(&self, path: &Path) -> Result<DatabaseStats> {
        if path.exists() {
            return Err(format!(
                "refusing to overwrite existing database: {}",
                path.display()
            )
            .into());
        }
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY;",
        )?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(SCHEMA)?;
        transaction.execute(
            "INSERT INTO metadata(key, value) VALUES ('schema_version', '1'), ('authority', 'index.scip')",
            [],
        )?;

        let document_ids: BTreeMap<_, _> = self
            .raw
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| (document.relative_path.clone(), index as i64 + 1))
            .collect();
        {
            let mut statement = transaction.prepare(
                "INSERT INTO documents(id,path,language,position_encoding,text) VALUES (?,?,?,?,?)",
            )?;
            for (index, document) in self.raw.documents.iter().enumerate() {
                statement.execute(params![
                    index as i64 + 1,
                    document.relative_path,
                    document.language,
                    document.position_encoding.value(),
                    document.text,
                ])?;
            }
        }

        let symbol_ids: BTreeMap<SymbolId, i64> = self
            .symbols
            .keys()
            .cloned()
            .enumerate()
            .map(|(index, symbol)| (symbol, index as i64 + 1))
            .collect();
        {
            let mut statement = transaction.prepare(
                "INSERT INTO symbols(id,identity_document,scip_symbol,canonical,display_name,qualified_name,kinds_json) VALUES (?,?,?,?,?,?,?)",
            )?;
            for (symbol, id) in &symbol_ids {
                let view = self.symbol_view(symbol);
                statement.execute(params![
                    id,
                    symbol.document.as_deref().unwrap_or(""),
                    symbol.symbol,
                    symbol.canonical(),
                    view.display_name,
                    view.qualified_name,
                    serde_json::to_string(&view.kinds)?,
                ])?;
            }
        }

        let mut information_count = 0usize;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO symbol_information(symbol_id,document,source_index,external,documentation_json,kind,display_name,enclosing_symbol,signature_language,signature_text,raw_protobuf) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
            )?;
            for (symbol, data) in &self.symbols {
                let symbol_id = symbol_ids[symbol];
                for information in &data.information {
                    let raw = self
                        .raw_symbol_information(
                            information.document.as_deref(),
                            information.symbol_information_index,
                        )
                        .ok_or("missing raw symbol information")?
                        .write_to_bytes()?;
                    statement.execute(params![
                        symbol_id,
                        information.document,
                        information.symbol_information_index as i64,
                        information.external,
                        serde_json::to_string(&information.documentation)?,
                        information.kind,
                        information.display_name,
                        information.enclosing_symbol,
                        information.signature.as_ref().map(|value| &value.language),
                        information.signature.as_ref().map(|value| &value.text),
                        raw,
                    ])?;
                    information_count += 1;
                }
            }
        }

        let mut definition_count = 0usize;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO occurrences(document_id,occurrence_index,symbol_id,owner_symbol_id,start_line,start_character,end_line,end_character,range_source,legacy_range_json,enclosing_start_line,enclosing_start_character,enclosing_end_line,enclosing_end_character,enclosing_range_source,legacy_enclosing_range_json,roles,role_names_json,position_encoding,syntax_kind,override_documentation_json,diagnostic_count,provenance,raw_protobuf) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )?;
            for occurrence in &self.occurrences {
                let range = range_columns(occurrence.range);
                let enclosing = range_columns(occurrence.enclosing_range);
                let raw = self
                    .raw_occurrence(&occurrence.document, occurrence.occurrence_index)
                    .ok_or("missing raw occurrence")?
                    .write_to_bytes()?;
                statement.execute(params![
                    document_ids[&occurrence.document],
                    occurrence.occurrence_index as i64,
                    occurrence.symbol.as_ref().map(|value| symbol_ids[value]),
                    occurrence.owner.as_ref().map(|value| symbol_ids[value]),
                    range.0,
                    range.1,
                    range.2,
                    range.3,
                    enum_json(&occurrence.range_source)?,
                    serde_json::to_string(&occurrence.legacy_range)?,
                    enclosing.0,
                    enclosing.1,
                    enclosing.2,
                    enclosing.3,
                    enum_json(&occurrence.enclosing_range_source)?,
                    serde_json::to_string(&occurrence.legacy_enclosing_range)?,
                    occurrence.symbol_roles,
                    serde_json::to_string(&occurrence.role_names)?,
                    occurrence.position_encoding,
                    occurrence.syntax_kind,
                    serde_json::to_string(&occurrence.override_documentation)?,
                    occurrence.diagnostic_count as i64,
                    occurrence.provenance,
                    raw,
                ])?;
                definition_count += usize::from(occurrence.is_definition());
            }
        }

        {
            let mut statement = transaction.prepare(
                "INSERT INTO relationships(source_symbol_id,target_symbol_id,document,symbol_information_index,relationship_index,is_reference,is_implementation,is_type_definition,is_definition,provenance,raw_protobuf) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
            )?;
            for relationship in &self.relationships {
                let raw = self
                    .raw_relationship(
                        relationship.document.as_deref(),
                        relationship.symbol_information_index,
                        relationship.relationship_index,
                    )
                    .ok_or("missing raw relationship")?
                    .write_to_bytes()?;
                statement.execute(params![
                    symbol_ids[&relationship.source],
                    symbol_ids[&relationship.target],
                    relationship.document,
                    relationship.symbol_information_index as i64,
                    relationship.relationship_index as i64,
                    relationship.is_reference,
                    relationship.is_implementation,
                    relationship.is_type_definition,
                    relationship.is_definition,
                    relationship.provenance,
                    raw,
                ])?;
            }
        }
        transaction.commit()?;
        connection.execute_batch("PRAGMA optimize;")?;
        Ok(DatabaseStats {
            documents: document_ids.len(),
            symbols: symbol_ids.len(),
            symbol_information: information_count,
            occurrences: self.occurrences.len(),
            definitions: definition_count,
            relationships: self.relationships.len(),
        })
    }
}

pub struct SqlDatabase {
    connection: Connection,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReferenceKey {
    source: String,
    target: String,
    document: Option<String>,
    roles_json: String,
    provenance: String,
    relationship: Option<SqlRelationshipFlags>,
}

#[derive(Clone, Debug)]
struct ReferenceGroup {
    line: Option<i64>,
    column: Option<i64>,
    count: usize,
}

#[derive(Clone, Debug)]
struct ResolvedCandidate {
    id: i64,
    local: bool,
    view: SqlSymbolCandidate,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TestKey {
    document: String,
    target: String,
    match_kind: String,
    roles_json: String,
}

#[derive(Clone, Debug)]
struct TestProjection {
    match_kind: &'static str,
    depth: usize,
    path: Vec<String>,
}

#[derive(Clone, Debug)]
struct TestGroup {
    line: Option<i64>,
    column: Option<i64>,
    count: usize,
    depth: usize,
    path: Vec<String>,
}

impl SqlDatabase {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Self { connection })
    }

    pub fn stats(&self) -> Result<DatabaseStats> {
        let count = |table: &str| -> Result<usize> {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            let value: i64 = self.connection.query_row(&sql, [], |row| row.get(0))?;
            Ok(value as usize)
        };
        Ok(DatabaseStats {
            documents: count("documents")?,
            symbols: count("symbols")?,
            symbol_information: count("symbol_information")?,
            occurrences: count("occurrences")?,
            definitions: self.connection.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM occurrences WHERE roles & 1 != 0",
                [],
                |row| row.get(0),
            )? as usize,
            relationships: count("relationships")?,
        })
    }

    pub fn refs(
        &self,
        selector: &str,
        direction: RefDirection,
        path_prefix: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<(String, SqlReferencePage)> {
        let (symbol_id, resolved) = self.resolve(selector)?;
        let mut groups = BTreeMap::<ReferenceKey, ReferenceGroup>::new();
        if matches!(direction, RefDirection::Incoming | RefDirection::Both) {
            self.occurrence_groups(symbol_id, false, path_prefix, &mut groups)?;
            self.relationship_groups(symbol_id, false, path_prefix, &mut groups)?;
        }
        if matches!(direction, RefDirection::Outgoing | RefDirection::Both) {
            self.occurrence_groups(symbol_id, true, path_prefix, &mut groups)?;
            self.relationship_groups(symbol_id, true, path_prefix, &mut groups)?;
        }
        let mut items: Vec<_> = groups
            .into_iter()
            .map(|(key, group)| SqlReferenceSummary {
                low_signal: key.target.starts_with(&format!("{}.", key.source)),
                source: key.source,
                target: key.target,
                document: key.document,
                line: group.line.map(|value| value + 1),
                column: group.column.map(|value| value + 1),
                roles: serde_json::from_str(&key.roles_json).unwrap_or_default(),
                provenance: key.provenance,
                occurrences: group.count,
                relationship: key.relationship,
            })
            .collect();
        items.sort_by_key(|item| {
            (
                item.low_signal,
                item.source.clone(),
                item.target.clone(),
                item.document.clone(),
                item.line,
            )
        });
        let total = items.len();
        let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        let returned = items.len();
        let next_offset = (offset + returned < total).then_some(offset + returned);
        Ok((
            resolved,
            SqlReferencePage {
                total,
                offset,
                returned,
                truncated: next_offset.is_some(),
                next_offset,
                items,
            },
        ))
    }

    pub fn tests(
        &self,
        selector: &str,
        path_prefix: &str,
        max_depth: usize,
        offset: usize,
        limit: usize,
    ) -> Result<(String, SqlTestPage)> {
        let (symbol_id, resolved) = self.resolve(selector)?;
        let mut targets = BTreeMap::new();
        let mut seen = BTreeSet::from([symbol_id]);
        let mut queue = VecDeque::from([(symbol_id, 0, vec![resolved.clone()])]);
        while let Some((current, depth, path)) = queue.pop_front() {
            self.add_test_projection(current, depth, &path, &mut targets)?;
            if depth >= max_depth {
                continue;
            }
            for (target, name) in self.callable_references(current)? {
                if seen.insert(target) {
                    let mut next_path = path.clone();
                    next_path.push(name);
                    queue.push_back((target, depth + 1, next_path));
                }
            }
        }

        let mut query = self.connection.prepare(
            "SELECT d.path,o.start_line,o.start_character,o.role_names_json,s.qualified_name
             FROM occurrences o
             JOIN documents d ON d.id=o.document_id
             JOIN symbols s ON s.id=o.symbol_id
             WHERE o.symbol_id=? AND d.path LIKE ? || '%'",
        )?;
        let mut groups = BTreeMap::<TestKey, TestGroup>::new();
        for (target, projection) in targets {
            let rows = query.query_map(params![target, path_prefix], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (document, line, column, roles_json, target) = row?;
                let key = TestKey {
                    document,
                    target,
                    match_kind: projection.match_kind.to_owned(),
                    roles_json,
                };
                let group = groups.entry(key).or_insert(TestGroup {
                    line,
                    column,
                    count: 0,
                    depth: projection.depth,
                    path: projection.path.clone(),
                });
                group.line = group.line.min(line).or(group.line).or(line);
                group.column = group.column.min(column).or(group.column).or(column);
                group.count += 1;
            }
        }

        let mut items: Vec<_> = groups
            .into_iter()
            .map(|(key, group)| SqlTestSummary {
                document: key.document,
                line: group.line.map(|value| value + 1),
                column: group.column.map(|value| value + 1),
                target: key.target,
                match_kind: key.match_kind,
                roles: serde_json::from_str(&key.roles_json).unwrap_or_default(),
                occurrences: group.count,
                depth: group.depth,
                path: group.path,
            })
            .collect();
        items.sort_by(|left, right| {
            (left.depth, &left.document, &left.target, &left.match_kind).cmp(&(
                right.depth,
                &right.document,
                &right.target,
                &right.match_kind,
            ))
        });
        let total = items.len();
        let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        let returned = items.len();
        let next_offset = (offset + returned < total).then_some(offset + returned);
        Ok((
            resolved,
            SqlTestPage {
                total,
                offset,
                returned,
                truncated: next_offset.is_some(),
                next_offset,
                items,
            },
        ))
    }

    fn add_test_projection(
        &self,
        symbol_id: i64,
        depth: usize,
        path: &[String],
        targets: &mut BTreeMap<i64, TestProjection>,
    ) -> Result<()> {
        let owner = self.class_owner(symbol_id)?;
        let root = owner.unwrap_or(symbol_id);
        insert_projection(targets, symbol_id, "symbol", depth, path);
        if owner.is_some() {
            insert_projection(targets, root, "owner", depth, path);
        }

        if owner.is_none() && self.has_kind(root, symbol_information::Kind::Class as i32)? {
            let mut members = self.connection.prepare(
                "SELECT DISTINCT symbol_id FROM occurrences
                 WHERE owner_symbol_id=? AND symbol_id<>owner_symbol_id AND roles & 1 != 0",
            )?;
            for member in members
                .query_map([root], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
            {
                insert_projection(targets, member, "member", depth, path);
            }
        }

        let mut subtypes = self.connection.prepare(
            "SELECT DISTINCT source_symbol_id FROM relationships
             WHERE target_symbol_id=? AND (is_implementation OR is_type_definition)",
        )?;
        for subtype in subtypes
            .query_map([root], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
        {
            insert_projection(targets, subtype, "subtype", depth, path);
        }
        Ok(())
    }

    fn callable_references(&self, source: i64) -> Result<Vec<(i64, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT target.id,target.qualified_name,target.kinds_json
             FROM occurrences occurrence
             JOIN symbols target ON target.id=occurrence.symbol_id
             WHERE occurrence.owner_symbol_id=? AND occurrence.roles & 1 = 0
               AND occurrence.roles & ? != 0
               AND EXISTS (
                 SELECT 1 FROM occurrences definition
                 WHERE definition.symbol_id=target.id AND definition.roles & 1 != 0
               )
             ORDER BY target.qualified_name",
        )?;
        let rows = statement.query_map(params![source, SymbolRole::ReadAccess as i32], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, name, kinds) = row?;
            let kinds: Vec<i32> = serde_json::from_str(&kinds).unwrap_or_default();
            if kinds.contains(&(symbol_information::Kind::Function as i32))
                || kinds.contains(&(symbol_information::Kind::Method as i32))
            {
                result.push((id, name));
            }
        }
        Ok(result)
    }

    fn has_kind(&self, symbol_id: i64, kind: i32) -> Result<bool> {
        let kinds: String = self.connection.query_row(
            "SELECT kinds_json FROM symbols WHERE id=?",
            [symbol_id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str::<Vec<i32>>(&kinds)
            .unwrap_or_default()
            .contains(&kind))
    }

    fn class_owner(&self, symbol_id: i64) -> Result<Option<i64>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT owner.id,owner.kinds_json
             FROM occurrences definition
             JOIN symbols owner ON owner.id=definition.owner_symbol_id
             WHERE definition.symbol_id=? AND definition.roles & 1 != 0",
        )?;
        let owners: Vec<(i64, String)> = statement
            .query_map([symbol_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(owners.into_iter().find_map(|(id, kinds)| {
            serde_json::from_str::<Vec<i32>>(&kinds)
                .unwrap_or_default()
                .contains(&(symbol_information::Kind::Class as i32))
                .then_some(id)
        }))
    }

    fn resolve(&self, selector: &str) -> Result<(i64, String)> {
        let alias = selector
            .rsplit_once('#')
            .filter(|_| !selector.contains(' '))
            .map_or_else(
                || selector.to_owned(),
                |(owner, member)| format!("{owner}.{member}"),
            );
        let exact = self.candidates(
            "canonical=?1 OR scip_symbol=?1 OR qualified_name=?1 OR qualified_name=?2",
            selector,
            &alias,
        )?;
        if let [candidate] = exact.as_slice() {
            return Ok((candidate.id, candidate.view.qualified_name.clone()));
        }

        let candidates = if exact.is_empty() {
            self.candidates(
                "substr(qualified_name, -length(?2)-1)='.' || ?2 OR display_name=?2",
                selector,
                &alias,
            )?
        } else {
            exact
        };
        let mut resolved = BTreeMap::new();
        for candidate in candidates {
            let candidate = self.import_target(&candidate)?.unwrap_or(candidate);
            resolved.entry(candidate.id).or_insert(candidate);
        }
        if resolved.values().any(|candidate| !candidate.local) {
            resolved.retain(|_, candidate| !candidate.local);
        }
        match resolved.into_values().collect::<Vec<_>>().as_slice() {
            [candidate] => Ok((candidate.id, candidate.view.qualified_name.clone())),
            [] => Err(SqlSelectionError::NotFound {
                selector: selector.to_owned(),
            }
            .into()),
            candidates => Err(SqlSelectionError::Ambiguous {
                selector: selector.to_owned(),
                candidates: candidates
                    .iter()
                    .take(8)
                    .map(|candidate| candidate.view.clone())
                    .collect(),
            }
            .into()),
        }
    }

    fn candidates(
        &self,
        predicate: &str,
        selector: &str,
        alias: &str,
    ) -> Result<Vec<ResolvedCandidate>> {
        let sql = format!(
            "SELECT s.id,s.identity_document,s.canonical,s.qualified_name,s.display_name,s.kinds_json,
                    (SELECT MIN(d.path) FROM occurrences o
                     JOIN documents d ON d.id=o.document_id
                     WHERE o.symbol_id=s.id AND o.roles & 1 != 0)
             FROM symbols s WHERE ({predicate}) AND ?2 IS NOT NULL ORDER BY s.canonical"
        );
        let mut statement = self.connection.prepare(&sql)?;
        Ok(statement
            .query_map(params![selector, alias], |row| {
                let identity_document: String = row.get(1)?;
                let kinds_json: String = row.get(5)?;
                Ok(ResolvedCandidate {
                    id: row.get(0)?,
                    local: !identity_document.is_empty(),
                    view: SqlSymbolCandidate {
                        canonical: row.get(2)?,
                        qualified_name: row.get(3)?,
                        display_name: row.get(4)?,
                        kinds: serde_json::from_str(&kinds_json).unwrap_or_default(),
                        definition: row.get(6)?,
                    },
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn import_target(&self, candidate: &ResolvedCandidate) -> Result<Option<ResolvedCandidate>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT target.symbol_id
             FROM occurrences alias
             JOIN symbols alias_symbol ON alias_symbol.id=alias.symbol_id
             JOIN occurrences target
               ON target.document_id=alias.document_id
              AND target.start_line IS alias.start_line
              AND target.start_character IS alias.start_character
              AND target.end_line IS alias.end_line
              AND target.end_character IS alias.end_character
             JOIN symbols target_symbol ON target_symbol.id=target.symbol_id
             WHERE alias.symbol_id=? AND alias_symbol.identity_document<>''
               AND alias.roles & 3 = 3 AND target.roles & 2 != 0
               AND target_symbol.identity_document='' AND target.symbol_id<>alias.symbol_id",
        )?;
        let targets: Vec<i64> = statement
            .query_map([candidate.id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let [target] = targets.as_slice() else {
            return Ok(None);
        };
        Ok(self
            .candidates("s.id=?1", &target.to_string(), "")?
            .into_iter()
            .next())
    }

    fn occurrence_groups(
        &self,
        symbol_id: i64,
        outgoing: bool,
        path_prefix: Option<&str>,
        groups: &mut BTreeMap<ReferenceKey, ReferenceGroup>,
    ) -> Result<()> {
        let column = if outgoing {
            "o.owner_symbol_id"
        } else {
            "o.symbol_id"
        };
        let sql = format!(
            "SELECT source.qualified_name,target.qualified_name,d.path,o.start_line,o.start_character,o.role_names_json,o.provenance
             FROM occurrences o
             JOIN symbols source ON source.id=o.owner_symbol_id
             JOIN symbols target ON target.id=o.symbol_id
             JOIN documents d ON d.id=o.document_id
             WHERE {column}=? AND o.roles & 1 = 0"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([symbol_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        for row in rows {
            let (source, target, document, line, column, roles_json, provenance) = row?;
            if path_prefix.is_some_and(|prefix| !document.starts_with(prefix)) {
                continue;
            }
            absorb_group(
                groups,
                ReferenceKey {
                    source,
                    target,
                    document: Some(document),
                    roles_json,
                    provenance,
                    relationship: None,
                },
                line,
                column,
            );
        }
        Ok(())
    }

    fn relationship_groups(
        &self,
        symbol_id: i64,
        outgoing: bool,
        path_prefix: Option<&str>,
        groups: &mut BTreeMap<ReferenceKey, ReferenceGroup>,
    ) -> Result<()> {
        let column = if outgoing {
            "r.source_symbol_id"
        } else {
            "r.target_symbol_id"
        };
        let sql = format!(
            "SELECT source.qualified_name,target.qualified_name,r.document,r.is_reference,r.is_implementation,r.is_type_definition,r.is_definition,r.provenance
             FROM relationships r
             JOIN symbols source ON source.id=r.source_symbol_id
             JOIN symbols target ON target.id=r.target_symbol_id
             WHERE {column}=?"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([symbol_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                SqlRelationshipFlags {
                    is_reference: row.get(3)?,
                    is_implementation: row.get(4)?,
                    is_type_definition: row.get(5)?,
                    is_definition: row.get(6)?,
                },
                row.get::<_, String>(7)?,
            ))
        })?;
        for row in rows {
            let (source, target, document, relationship, provenance) = row?;
            if path_prefix.is_some_and(|prefix| {
                document
                    .as_deref()
                    .is_none_or(|document| !document.starts_with(prefix))
            }) {
                continue;
            }
            absorb_group(
                groups,
                ReferenceKey {
                    source,
                    target,
                    document,
                    roles_json: "[]".to_owned(),
                    provenance,
                    relationship: Some(relationship),
                },
                None,
                None,
            );
        }
        Ok(())
    }
}

fn absorb_group(
    groups: &mut BTreeMap<ReferenceKey, ReferenceGroup>,
    key: ReferenceKey,
    line: Option<i64>,
    column: Option<i64>,
) {
    let group = groups.entry(key).or_insert(ReferenceGroup {
        line,
        column,
        count: 0,
    });
    group.line = group.line.min(line).or(group.line).or(line);
    group.column = group.column.min(column).or(group.column).or(column);
    group.count += 1;
}

fn insert_projection(
    targets: &mut BTreeMap<i64, TestProjection>,
    symbol_id: i64,
    match_kind: &'static str,
    depth: usize,
    path: &[String],
) {
    let replace = targets
        .get(&symbol_id)
        .is_none_or(|current| depth < current.depth);
    if replace {
        targets.insert(
            symbol_id,
            TestProjection {
                match_kind,
                depth,
                path: path.to_vec(),
            },
        );
    }
}

fn range_columns(
    range: Option<SourceRange>,
) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    range.map_or((None, None, None, None), |range| {
        (
            Some(range.start.line.into()),
            Some(range.start.character.into()),
            Some(range.end.line.into()),
            Some(range.end.character.into()),
        )
    })
}

fn enum_json(value: &impl Serialize) -> Result<String> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_owned())
}
