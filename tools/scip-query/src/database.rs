//! Lossless normalized SQLite cache and high-signal reference projections.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use protobuf::Message;
use rusqlite::{Connection, OpenFlags, params};
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

    fn resolve(&self, selector: &str) -> Result<(i64, String)> {
        let alias = selector
            .rsplit_once('#')
            .filter(|_| !selector.contains(' '))
            .map_or_else(
                || selector.to_owned(),
                |(owner, member)| format!("{owner}.{member}"),
            );
        let mut statement = self.connection.prepare(
            "SELECT id, qualified_name FROM symbols
             WHERE canonical=?1 OR scip_symbol=?1 OR qualified_name=?1 OR qualified_name=?2
                OR qualified_name LIKE '%.' || ?2 OR display_name=?2
             ORDER BY CASE WHEN qualified_name=?2 THEN 0 ELSE 1 END, canonical",
        )?;
        let candidates: Vec<(i64, String)> = statement
            .query_map(params![selector, alias], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        match candidates.as_slice() {
            [candidate] => Ok(candidate.clone()),
            [] => Err(format!("symbol not found: {selector}").into()),
            _ => Err(format!(
                "ambiguous symbol {selector}: {}",
                candidates
                    .iter()
                    .take(8)
                    .map(|value| value.1.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into()),
        }
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
