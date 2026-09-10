//! Bounded, offline LSP-style queries over an in-memory SCIP index.
//!
//! [`QueryIndex`] retains the decoded [`scip::types::Index`] as its source of
//! truth. References remain SCIP references; calls are exposed only when a
//! fingerprint-matched producer sidecar supplies resolved callee positions.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use protobuf::{Enum, Message};
use scip::types::{Index, Occurrence, PositionEncoding, SymbolInformation, SymbolRole, occurrence};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Default cap used by composite responses such as [`QueryIndex::context`].
pub const DEFAULT_LIMIT: usize = 100;
const MAX_SNIPPET_LINES: usize = 12;

/// Stable symbol identity. SCIP local symbols are qualified by their document.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SymbolId {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    pub symbol: String,
}

impl SymbolId {
    /// Construct the identity SCIP assigns to `symbol` in `document`.
    pub fn new(document: &str, symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        Self {
            document: symbol.starts_with("local ").then(|| document.to_owned()),
            symbol,
        }
    }

    /// A deterministic, copy-paste-safe identity string.
    pub fn canonical(&self) -> String {
        match &self.document {
            Some(path) => format!("{path}::{symbol}", symbol = self.symbol),
            None => self.symbol.clone(),
        }
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// A zero-based half-open SCIP source position.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Position {
    pub line: i32,
    pub character: i32,
}

/// A zero-based half-open SCIP source range.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
}

impl SourceRange {
    fn parse(values: &[i32]) -> Option<Self> {
        match values {
            [line, start, end] => Some(Self {
                start: Position {
                    line: *line,
                    character: *start,
                },
                end: Position {
                    line: *line,
                    character: *end,
                },
            }),
            [start_line, start_character, end_line, end_character] => Some(Self {
                start: Position {
                    line: *start_line,
                    character: *start_character,
                },
                end: Position {
                    line: *end_line,
                    character: *end_character,
                },
            }),
            _ => None,
        }
    }

    fn contains_range(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    fn contains_position(self, point: Position) -> bool {
        self.start <= point && point < self.end
    }

    fn specificity(self) -> (Reverse<Position>, Position) {
        // For nested ranges, the innermost starts latest and ends earliest.
        (Reverse(self.start), self.end)
    }
}

/// Which wire representation supplied a normalized range.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeSource {
    Typed,
    Legacy,
    Missing,
}

/// A deterministic bounded list. `total` is measured before truncation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Page<T> {
    pub total: usize,
    pub returned: usize,
    pub truncated: bool,
    pub items: Vec<T>,
}

impl<T> Page<T> {
    fn new(mut items: Vec<T>, limit: usize) -> Self {
        let total = items.len();
        items.truncate(limit);
        Self {
            total,
            returned: items.len(),
            truncated: total > items.len(),
            items,
        }
    }
}

/// Signature metadata retained from `SymbolInformation`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SignatureView {
    pub language: String,
    pub text: String,
}

/// Definition or reference evidence without losing its raw-index address.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OccurrenceView {
    pub document: String,
    pub occurrence_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<SymbolId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
    pub range_source: RangeSource,
    pub legacy_range: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_range: Option<SourceRange>,
    pub enclosing_range_source: RangeSource,
    pub legacy_enclosing_range: Vec<i32>,
    pub symbol_roles: i32,
    pub role_names: Vec<&'static str>,
    pub position_encoding: i32,
    pub syntax_kind: i32,
    pub override_documentation: Vec<String>,
    pub diagnostic_count: usize,
    pub provenance: &'static str,
}

impl OccurrenceView {
    pub fn is_definition(&self) -> bool {
        self.symbol_roles & SymbolRole::Definition as i32 != 0
    }
}

/// All four independent flags on one SCIP relationship.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RelationshipView {
    pub source: SymbolId,
    pub target: SymbolId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    pub symbol_information_index: usize,
    pub relationship_index: usize,
    pub is_reference: bool,
    pub is_implementation: bool,
    pub is_type_definition: bool,
    pub is_definition: bool,
    pub provenance: &'static str,
}

/// Compact symbol metadata used by discovery responses.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SymbolView {
    pub id: SymbolId,
    pub display_name: String,
    pub qualified_name: String,
    pub kinds: Vec<i32>,
    pub definition_count: usize,
    pub occurrence_count: usize,
    pub relationship_count: usize,
}

/// Lossless query-facing context for one symbol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SymbolContext {
    pub symbol: SymbolView,
    pub documentation: Vec<String>,
    pub signatures: Vec<SignatureView>,
    pub symbol_information: Page<SymbolInformationView>,
    pub definitions: Page<OccurrenceView>,
    pub occurrences: Page<OccurrenceView>,
    pub relationships: Page<RelationshipView>,
    pub snippets: Page<SourceSnippet>,
    pub snippet_failures: Page<SnippetFailure>,
}

/// A definition whose source excerpt could not be loaded.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SnippetFailure {
    pub document: String,
    pub occurrence_index: usize,
    pub error: String,
}

/// One unmerged `SymbolInformation` record. Its indices address the raw record.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SymbolInformationView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    pub symbol_information_index: usize,
    pub external: bool,
    pub symbol: SymbolId,
    pub documentation: Vec<String>,
    pub kind: i32,
    pub display_name: String,
    pub enclosing_symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureView>,
    pub relationship_count: usize,
}

/// A bounded source excerpt with 1-based display line numbers.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceSnippet {
    pub document: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub returned_lines: usize,
    pub truncated: bool,
    pub text: String,
    pub provenance: &'static str,
}

/// Exact resolution always exposes ambiguity rather than picking a candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Resolution {
    Found {
        symbol: SymbolView,
    },
    Ambiguous {
        query: String,
        candidates: Page<SymbolView>,
    },
    NotFound {
        query: String,
    },
}

/// Direction for semantic reference traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefDirection {
    Incoming,
    Outgoing,
    Both,
}

/// Edge evidence. Occurrences remain references; no call inference is made.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceEvidence {
    Occurrence { occurrence: OccurrenceView },
    Relationship { relationship: RelationshipView },
}

/// One directed semantic edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReferenceView {
    pub source: SymbolId,
    pub target: SymbolId,
    pub evidence: ReferenceEvidence,
}

/// A resolved reference proven by the producer to occupy a call's callee.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CallView {
    pub caller: SymbolId,
    pub callee: SymbolId,
    pub document: String,
    pub range: SourceRange,
    pub provenance: &'static str,
}

/// A direct lexical member and the ownership evidence that produced it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct MemberView {
    pub symbol: SymbolView,
    pub definition: OccurrenceView,
    pub provenance: &'static str,
}

/// Errors at the protobuf, filesystem, or query boundary.
#[derive(Debug)]
pub enum QueryError {
    Protobuf(protobuf::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    UnknownDocument(String),
    UnknownSymbol(String),
    InvalidPosition { line: usize, column: usize },
    UnsupportedPositionEncoding(i32),
    UnsafePath(String),
    MissingCallFacts,
    InvalidFacts(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protobuf(error) => write!(f, "invalid SCIP protobuf: {error}"),
            Self::Json(error) => write!(f, "invalid ty facts JSON: {error}"),
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::UnknownDocument(path) => write!(f, "document not found: {path}"),
            Self::UnknownSymbol(symbol) => write!(f, "symbol not found: {symbol}"),
            Self::InvalidPosition { line, column } => {
                write!(f, "line and column must be 1-based (got {line}:{column})")
            }
            Self::UnsupportedPositionEncoding(encoding) => {
                write!(f, "unsupported SCIP position encoding: {encoding}")
            }
            Self::UnsafePath(path) => write!(f, "document path escapes the project root: {path}"),
            Self::MissingCallFacts => write!(
                f,
                "call hierarchy requires a synchronized ty facts sidecar (--facts PATH)"
            ),
            Self::InvalidFacts(message) => write!(f, "invalid ty facts: {message}"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<protobuf::Error> for QueryError {
    fn from(value: protobuf::Error) -> Self {
        Self::Protobuf(value)
    }
}

impl From<std::io::Error> for QueryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for QueryError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Deserialize)]
struct TyFacts {
    format: String,
    version: u32,
    index_sha256: String,
    facts: Vec<TyFact>,
}

#[derive(Deserialize)]
struct TyFact {
    kind: String,
    document: String,
    range: Vec<i32>,
    enclosing_symbol: String,
    symbol: String,
}

#[derive(Clone, Debug, Default)]
struct SymbolData {
    display_names: BTreeSet<String>,
    qualified_names: BTreeSet<String>,
    kinds: BTreeSet<i32>,
    documentation: BTreeSet<String>,
    signatures: BTreeSet<SignatureView>,
    occurrences: Vec<usize>,
    definitions: Vec<usize>,
    relationships: Vec<usize>,
    information: Vec<SymbolInformationView>,
}

/// Decoded SCIP plus deterministic, in-memory navigation indexes.
pub struct QueryIndex {
    raw: Index,
    index_sha256: String,
    root: Option<PathBuf>,
    documents: BTreeMap<String, usize>,
    symbols: BTreeMap<SymbolId, SymbolData>,
    occurrences: Vec<OccurrenceView>,
    relationships: Vec<RelationshipView>,
    outgoing: BTreeMap<SymbolId, Vec<ReferenceView>>,
    incoming: BTreeMap<SymbolId, Vec<ReferenceView>>,
    calls: Vec<CallView>,
    outgoing_calls: BTreeMap<SymbolId, Vec<CallView>>,
    incoming_calls: BTreeMap<SymbolId, Vec<CallView>>,
    call_facts_loaded: bool,
}

impl QueryIndex {
    /// Decode a `.scip` file and build its navigation indexes.
    pub fn load(path: impl AsRef<Path>, root: Option<PathBuf>) -> Result<Self, QueryError> {
        Self::from_bytes(&fs::read(path)?, root)
    }

    /// Decode a `.scip` file and join a fingerprint-matched ty facts sidecar.
    pub fn load_with_facts(
        path: impl AsRef<Path>,
        facts: impl AsRef<Path>,
        root: Option<PathBuf>,
    ) -> Result<Self, QueryError> {
        let mut index = Self::load(path, root)?;
        index.load_facts(&fs::read(facts)?)?;
        Ok(index)
    }

    /// Decode SCIP bytes and build their navigation indexes.
    pub fn from_bytes(bytes: &[u8], root: Option<PathBuf>) -> Result<Self, QueryError> {
        let mut index = Self::from_index(Index::parse_from_bytes(bytes)?, root)?;
        index.index_sha256 = format!("{:x}", Sha256::digest(bytes));
        Ok(index)
    }

    /// Build navigation indexes while retaining `raw` unchanged as authority.
    pub fn from_index(raw: Index, root: Option<PathBuf>) -> Result<Self, QueryError> {
        let index_sha256 = format!("{:x}", Sha256::digest(raw.write_to_bytes()?));
        let documents = raw
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| (document.relative_path.clone(), index))
            .collect();
        let mut this = Self {
            raw,
            index_sha256,
            root,
            documents,
            symbols: BTreeMap::new(),
            occurrences: Vec::new(),
            relationships: Vec::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            calls: Vec::new(),
            outgoing_calls: BTreeMap::new(),
            incoming_calls: BTreeMap::new(),
            call_facts_loaded: false,
        };
        this.build();
        Ok(this)
    }

    /// The complete decoded protobuf. Query projections never replace it.
    pub fn raw(&self) -> &Index {
        &self.raw
    }

    /// Address an occurrence without copying or serializing its full protobuf.
    pub fn raw_occurrence(&self, document: &str, index: usize) -> Option<&Occurrence> {
        self.documents
            .get(document)
            .and_then(|document| self.raw.documents[*document].occurrences.get(index))
    }

    /// Address a document (`Some`) or external (`None`) symbol-information record.
    pub fn raw_symbol_information(
        &self,
        document: Option<&str>,
        index: usize,
    ) -> Option<&SymbolInformation> {
        match document {
            Some(path) => self
                .documents
                .get(path)
                .and_then(|document| self.raw.documents[*document].symbols.get(index)),
            None => self.raw.external_symbols.get(index),
        }
    }

    /// Address a relationship through its containing symbol-information record.
    pub fn raw_relationship(
        &self,
        document: Option<&str>,
        symbol_index: usize,
        relationship_index: usize,
    ) -> Option<&scip::types::Relationship> {
        self.raw_symbol_information(document, symbol_index)?
            .relationships
            .get(relationship_index)
    }

    /// Case-insensitive substring discovery over names and SCIP identities.
    pub fn find(&self, query: &str, limit: usize) -> Page<SymbolView> {
        self.find_in(query, None, limit)
    }

    /// Discovery optionally restricted to symbols evidenced in `document`.
    pub fn find_in(&self, query: &str, document: Option<&str>, limit: usize) -> Page<SymbolView> {
        let needle = query.to_lowercase();
        let mut matches: Vec<_> = self
            .symbols
            .iter()
            .filter(|(id, data)| {
                document.is_none_or(|path| symbol_has_document(self, id, path))
                    && (id.symbol.to_lowercase().contains(&needle)
                        || data
                            .display_names
                            .iter()
                            .any(|name| name.to_lowercase().contains(&needle))
                        || data
                            .qualified_names
                            .iter()
                            .any(|name| name.to_lowercase().contains(&needle)))
            })
            .map(|(id, _)| self.symbol_view(id))
            .collect();
        matches.sort_by_key(|symbol| find_rank(symbol, &needle));
        Page::new(matches, limit)
    }

    /// Resolve an exact SCIP identity, canonical local identity, display name,
    /// qualified descriptor name, or `document:name` query.
    pub fn resolve(&self, query: &str, document: Option<&str>, limit: usize) -> Resolution {
        let exact: Vec<_> = self
            .symbols
            .keys()
            .filter(|id| {
                id.canonical() == query
                    || (id.symbol == query
                        && document.is_none_or(|path| symbol_has_document(self, id, path)))
            })
            .map(|id| self.symbol_view(id))
            .collect();
        if !exact.is_empty() {
            return resolution(query, exact, limit);
        }
        let (document, query) = self.query_parts(query, document);
        let mut candidates: Vec<_> = self
            .symbols
            .iter()
            .filter(|(id, data)| {
                document.is_none_or(|path| {
                    id.document.as_deref().is_none_or(|local| local == path)
                        && symbol_has_document(self, id, path)
                }) && (id.symbol == query
                    || id.canonical() == query
                    || data.display_names.contains(query)
                    || data.qualified_names.iter().any(|name| {
                        name == query
                            || name
                                .strip_suffix(query)
                                .is_some_and(|prefix| prefix.ends_with('.'))
                    }))
            })
            .map(|(id, _)| self.symbol_view(id))
            .collect();
        candidates.sort();
        candidates.dedup();
        resolution(query, candidates, limit)
    }

    /// Return every occurrence whose preferred SCIP range contains a 1-based
    /// editor position. Columns are 1-based UTF-8 byte offsets, matching
    /// ripgrep, and are converted to the document's SCIP position encoding.
    pub fn at(
        &self,
        path: &str,
        line: usize,
        column: Option<usize>,
        limit: usize,
    ) -> Result<Page<OccurrenceView>, QueryError> {
        if line == 0 || column == Some(0) {
            return Err(QueryError::InvalidPosition {
                line,
                column: column.unwrap_or(0),
            });
        }
        let document = self.document(path)?;
        let encoding = document.position_encoding.enum_value().map_err(|_| {
            QueryError::UnsupportedPositionEncoding(document.position_encoding.value())
        })?;
        let text = match (column, encoding) {
            (None, _) => None,
            (
                Some(_),
                PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                | PositionEncoding::UnspecifiedPositionEncoding,
            ) => self.source_text(document).ok(),
            (
                Some(_),
                PositionEncoding::UTF16CodeUnitOffsetFromLineStart
                | PositionEncoding::UTF32CodeUnitOffsetFromLineStart,
            ) => Some(self.source_text(document)?),
        };
        let point = column
            .map(|column| {
                encoded_column(
                    text.as_deref(),
                    line,
                    column,
                    document.position_encoding.value(),
                )
                .map(|character| Position {
                    line: (line - 1) as i32,
                    character,
                })
            })
            .transpose()?;
        let mut items: Vec<_> = self
            .occurrences
            .iter()
            .filter(|item| item.document == path)
            .filter(|item| match column {
                None => item.range.is_some_and(|range| {
                    let query_line = (line - 1) as i32;
                    range.start.line <= query_line
                        && (query_line < range.end.line
                            || (query_line == range.end.line && range.end.character > 0))
                }),
                Some(_) => item
                    .range
                    .is_some_and(|range| range.contains_position(point.expect("column has point"))),
            })
            .cloned()
            .collect();
        items.sort_by_key(|item| {
            (
                item.range.map(SourceRange::specificity),
                !item.is_definition(),
                item.occurrence_index,
            )
        });
        Ok(Page::new(items, limit))
    }

    /// Rich symbol metadata, every kind and co-definition, all relationship
    /// flags, and bounded source excerpts around definitions.
    pub fn context(
        &self,
        id: &SymbolId,
        before: usize,
        after: usize,
    ) -> Result<SymbolContext, QueryError> {
        self.context_with_limit(id, before, after, DEFAULT_LIMIT)
    }

    /// [`Self::context`] with an explicit cap for each nested evidence list.
    pub fn context_with_limit(
        &self,
        id: &SymbolId,
        before: usize,
        after: usize,
        limit: usize,
    ) -> Result<SymbolContext, QueryError> {
        let data = self
            .symbols
            .get(id)
            .ok_or_else(|| QueryError::UnknownSymbol(id.to_string()))?;
        let occurrences: Vec<_> = data
            .occurrences
            .iter()
            .map(|index| self.occurrences[*index].clone())
            .collect();
        let definitions: Vec<_> = data
            .definitions
            .iter()
            .map(|index| self.occurrences[*index].clone())
            .collect();
        let relationships: Vec<_> = data
            .relationships
            .iter()
            .map(|index| self.relationships[*index].clone())
            .collect();
        let mut snippets = Vec::new();
        let mut snippet_failures = Vec::new();
        for definition in &definitions {
            match self.snippet(definition, before, after) {
                Ok(snippet) => snippets.push(snippet),
                Err(error) => snippet_failures.push(SnippetFailure {
                    document: definition.document.clone(),
                    occurrence_index: definition.occurrence_index,
                    error: error.to_string(),
                }),
            }
        }
        Ok(SymbolContext {
            symbol: self.symbol_view(id),
            documentation: data.documentation.iter().cloned().collect(),
            signatures: data.signatures.iter().cloned().collect(),
            symbol_information: Page::new(data.information.clone(), limit),
            definitions: Page::new(definitions, limit),
            occurrences: Page::new(occurrences, limit),
            relationships: Page::new(relationships, limit),
            snippets: Page::new(snippets, limit),
            snippet_failures: Page::new(snippet_failures, limit),
        })
    }

    /// Semantic reference evidence. Plain occurrences are deliberately never
    /// described as calls.
    pub fn refs(
        &self,
        id: &SymbolId,
        direction: RefDirection,
        limit: usize,
    ) -> Page<ReferenceView> {
        let mut items = Vec::new();
        if matches!(direction, RefDirection::Incoming | RefDirection::Both) {
            items.extend(self.incoming.get(id).into_iter().flatten().cloned());
        }
        if matches!(direction, RefDirection::Outgoing | RefDirection::Both) {
            items.extend(self.outgoing.get(id).into_iter().flatten().cloned());
        }
        items.sort();
        items.dedup();
        Page::new(items, limit)
    }

    /// Direct members derived from the smallest enclosing definition.
    pub fn members(&self, id: &SymbolId, limit: usize) -> Page<MemberView> {
        let mut items: Vec<_> = self
            .occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.is_definition() && occurrence.owner.as_ref() == Some(id)
            })
            .filter_map(|definition| {
                definition.symbol.as_ref().map(|symbol| MemberView {
                    symbol: self.symbol_view(symbol),
                    definition: definition.clone(),
                    provenance: "derived_lexical_ownership",
                })
            })
            .collect();
        items.sort();
        Page::new(items, limit)
    }

    /// Direct supertypes from SCIP implementation relationships.
    pub fn supertypes(&self, id: &SymbolId, limit: usize) -> Page<RelationshipView> {
        Page::new(
            self.relationships
                .iter()
                .filter(|relationship| relationship.source == *id && relationship.is_implementation)
                .cloned()
                .collect(),
            limit,
        )
    }

    /// Direct subtypes from SCIP implementation relationships.
    pub fn subtypes(&self, id: &SymbolId, limit: usize) -> Page<RelationshipView> {
        Page::new(
            self.relationships
                .iter()
                .filter(|relationship| relationship.target == *id && relationship.is_implementation)
                .cloned()
                .collect(),
            limit,
        )
    }

    /// Direct calls made by `id`, backed only by synchronized producer facts.
    pub fn callees(&self, id: &SymbolId, limit: usize) -> Result<Page<CallView>, QueryError> {
        self.require_call_facts()?;
        Ok(Page::new(
            self.outgoing_calls.get(id).cloned().unwrap_or_default(),
            limit,
        ))
    }

    /// Direct calls to `id`, backed only by synchronized producer facts.
    pub fn callers(&self, id: &SymbolId, limit: usize) -> Result<Page<CallView>, QueryError> {
        self.require_call_facts()?;
        Ok(Page::new(
            self.incoming_calls.get(id).cloned().unwrap_or_default(),
            limit,
        ))
    }

    fn require_call_facts(&self) -> Result<(), QueryError> {
        self.call_facts_loaded
            .then_some(())
            .ok_or(QueryError::MissingCallFacts)
    }

    fn load_facts(&mut self, bytes: &[u8]) -> Result<(), QueryError> {
        let sidecar: TyFacts = serde_json::from_slice(bytes)?;
        if sidecar.format != "ty-scip-facts" {
            return Err(QueryError::InvalidFacts(format!(
                "unsupported format {:?}",
                sidecar.format
            )));
        }
        if sidecar.version != 1 {
            return Err(QueryError::InvalidFacts(format!(
                "unsupported version {}",
                sidecar.version
            )));
        }
        if sidecar.index_sha256 != self.index_sha256 {
            return Err(QueryError::InvalidFacts(
                "sidecar fingerprint does not match the SCIP index".to_owned(),
            ));
        }
        for fact in sidecar.facts {
            if fact.kind != "callee_position" {
                return Err(QueryError::InvalidFacts(format!(
                    "unsupported fact kind {:?}",
                    fact.kind
                )));
            }
            let range = SourceRange::parse(&fact.range).ok_or_else(|| {
                QueryError::InvalidFacts(format!("invalid range {:?}", fact.range))
            })?;
            let caller = SymbolId::new(&fact.document, fact.enclosing_symbol);
            let callee = SymbolId::new(&fact.document, fact.symbol);
            if !self.symbols.contains_key(&caller) {
                return Err(QueryError::InvalidFacts(format!(
                    "unknown enclosing symbol {caller}"
                )));
            }
            if !self.symbols.contains_key(&callee) {
                return Err(QueryError::InvalidFacts(format!(
                    "unknown target symbol {callee}"
                )));
            }
            let occurrence_exists = self.occurrences.iter().any(|occurrence| {
                occurrence.document == fact.document
                    && occurrence.range == Some(range)
                    && occurrence.symbol.as_ref() == Some(&callee)
            });
            if !occurrence_exists {
                return Err(QueryError::InvalidFacts(format!(
                    "callee occurrence is absent at {}:{:?}",
                    fact.document, fact.range
                )));
            }
            self.calls.push(CallView {
                caller,
                callee,
                document: fact.document,
                range,
                provenance: "ty_scip_callee_position",
            });
        }
        self.calls.sort();
        self.calls.dedup();
        for call in &self.calls {
            self.outgoing_calls
                .entry(call.caller.clone())
                .or_default()
                .push(call.clone());
            self.incoming_calls
                .entry(call.callee.clone())
                .or_default()
                .push(call.clone());
        }
        self.call_facts_loaded = true;
        Ok(())
    }

    fn build(&mut self) {
        for document in &self.raw.documents {
            let id = document_id(&document.relative_path);
            let mut data = SymbolData::default();
            data.display_names.insert(document.relative_path.clone());
            data.qualified_names.insert(document.relative_path.clone());
            data.kinds.insert(16); // SCIP File
            self.symbols.insert(id, data);
        }
        self.collect_symbol_information();
        self.collect_occurrences();
        self.assign_owners();
        self.collect_relationships();
        self.build_edges();
    }

    fn collect_symbol_information(&mut self) {
        for document in &self.raw.documents {
            for (index, info) in document.symbols.iter().enumerate() {
                absorb_info(
                    &mut self.symbols,
                    &document.relative_path,
                    info,
                    Some(document.relative_path.clone()),
                    index,
                    false,
                );
            }
        }
        for (index, info) in self.raw.external_symbols.iter().enumerate() {
            absorb_info(&mut self.symbols, "", info, None, index, true);
        }
    }

    fn collect_occurrences(&mut self) {
        for document in &self.raw.documents {
            for (occurrence_index, occurrence) in document.occurrences.iter().enumerate() {
                let id = (!occurrence.symbol.is_empty())
                    .then(|| SymbolId::new(&document.relative_path, &occurrence.symbol));
                let index = self.occurrences.len();
                let (range, range_source) = normalized_range(occurrence, false);
                let (enclosing_range, enclosing_range_source) = normalized_range(occurrence, true);
                self.occurrences.push(OccurrenceView {
                    document: document.relative_path.clone(),
                    occurrence_index,
                    symbol: id.clone(),
                    owner: None,
                    range,
                    range_source,
                    legacy_range: occurrence.range.clone(),
                    enclosing_range,
                    enclosing_range_source,
                    legacy_enclosing_range: occurrence.enclosing_range.clone(),
                    symbol_roles: occurrence.symbol_roles,
                    role_names: role_names(occurrence.symbol_roles),
                    position_encoding: document.position_encoding.value(),
                    syntax_kind: occurrence.syntax_kind.value(),
                    override_documentation: occurrence.override_documentation.clone(),
                    diagnostic_count: occurrence.diagnostics.len(),
                    provenance: "scip_occurrence",
                });
                if let Some(id) = id {
                    let data = self.symbols.entry(id).or_default();
                    data.occurrences.push(index);
                    if occurrence.symbol_roles & SymbolRole::Definition as i32 != 0 {
                        data.definitions.push(index);
                    }
                }
            }
        }
    }

    fn assign_owners(&mut self) {
        let mut definitions = BTreeMap::<String, Vec<usize>>::new();
        for (index, occurrence) in self.occurrences.iter().enumerate() {
            if occurrence.is_definition() && occurrence.enclosing_range.is_some() {
                definitions
                    .entry(occurrence.document.clone())
                    .or_default()
                    .push(index);
            }
        }
        for indexes in definitions.values_mut() {
            indexes.sort_by_key(|index| {
                (
                    self.occurrences[*index]
                        .enclosing_range
                        .map(SourceRange::specificity),
                    *index,
                )
            });
        }
        for occurrence_index in 0..self.occurrences.len() {
            let occurrence = &self.occurrences[occurrence_index];
            let Some(range) = occurrence.range else {
                continue;
            };
            let lexical_owner = definitions
                .get(&occurrence.document)
                .into_iter()
                .flatten()
                .filter(|definition_index| **definition_index != occurrence_index)
                .filter_map(|definition_index| {
                    let definition = &self.occurrences[*definition_index];
                    let enclosing = definition.enclosing_range?;
                    (enclosing.contains_range(range)
                        && is_lexical_scope(definition.symbol.as_ref(), &self.symbols))
                    .then(|| {
                        (
                            enclosing.specificity(),
                            *definition_index,
                            definition.symbol.clone().expect("definition symbol"),
                        )
                    })
                })
                .min_by_key(|item| (item.0, item.1))
                .map(|item| item.2);
            let explicit_owner = occurrence
                .is_definition()
                .then(|| {
                    occurrence.symbol.as_ref().and_then(|symbol| {
                        self.symbols
                            .get(symbol)?
                            .information
                            .iter()
                            .find(|info| !info.enclosing_symbol.is_empty())
                            .map(|info| SymbolId::new(&occurrence.document, &info.enclosing_symbol))
                            .filter(|parent| self.symbols.contains_key(parent))
                    })
                })
                .flatten();
            self.occurrences[occurrence_index].owner = lexical_owner
                .or(explicit_owner)
                .or_else(|| Some(document_id(&occurrence.document)));
        }
    }

    fn collect_relationships(&mut self) {
        let mut records = Vec::new();
        for document in &self.raw.documents {
            for (symbol_information_index, info) in document.symbols.iter().enumerate() {
                for (relationship_index, relationship) in info.relationships.iter().enumerate() {
                    records.push((
                        Some(document.relative_path.clone()),
                        symbol_information_index,
                        relationship_index,
                        info.symbol.clone(),
                        relationship.clone(),
                    ));
                }
            }
        }
        for (symbol_information_index, info) in self.raw.external_symbols.iter().enumerate() {
            for (relationship_index, relationship) in info.relationships.iter().enumerate() {
                records.push((
                    None,
                    symbol_information_index,
                    relationship_index,
                    info.symbol.clone(),
                    relationship.clone(),
                ));
            }
        }
        for (document, symbol_information_index, relationship_index, source_symbol, relationship) in
            records
        {
            let identity_document = document.as_deref().unwrap_or("");
            let source = SymbolId::new(identity_document, source_symbol);
            let target = SymbolId::new(identity_document, &relationship.symbol);
            self.symbols.entry(source.clone()).or_default();
            self.symbols.entry(target.clone()).or_default();
            let index = self.relationships.len();
            self.relationships.push(RelationshipView {
                source: source.clone(),
                target: target.clone(),
                document,
                symbol_information_index,
                relationship_index,
                is_reference: relationship.is_reference,
                is_implementation: relationship.is_implementation,
                is_type_definition: relationship.is_type_definition,
                is_definition: relationship.is_definition,
                provenance: "scip_relationship",
            });
            self.symbols
                .get_mut(&source)
                .expect("inserted source")
                .relationships
                .push(index);
            self.symbols
                .get_mut(&target)
                .expect("inserted target")
                .relationships
                .push(index);
        }
    }

    fn build_edges(&mut self) {
        for occurrence in &self.occurrences {
            if occurrence.is_definition() {
                continue;
            }
            let (Some(source), Some(target)) =
                (occurrence.owner.clone(), occurrence.symbol.clone())
            else {
                continue;
            };
            let edge = ReferenceView {
                source: source.clone(),
                target: target.clone(),
                evidence: ReferenceEvidence::Occurrence {
                    occurrence: occurrence.clone(),
                },
            };
            self.outgoing.entry(source).or_default().push(edge.clone());
            self.incoming.entry(target).or_default().push(edge);
        }
        for relationship in &self.relationships {
            if !relationship.is_reference
                && !relationship.is_implementation
                && !relationship.is_type_definition
                && !relationship.is_definition
            {
                continue;
            }
            let edge = ReferenceView {
                source: relationship.source.clone(),
                target: relationship.target.clone(),
                evidence: ReferenceEvidence::Relationship {
                    relationship: relationship.clone(),
                },
            };
            self.outgoing
                .entry(edge.source.clone())
                .or_default()
                .push(edge.clone());
            self.incoming
                .entry(edge.target.clone())
                .or_default()
                .push(edge);
        }
        for edges in self.outgoing.values_mut() {
            edges.sort();
        }
        for edges in self.incoming.values_mut() {
            edges.sort();
        }
    }

    fn symbol_view(&self, id: &SymbolId) -> SymbolView {
        let data = self.symbols.get(id);
        let display_name = data
            .and_then(|data| data.display_names.first().cloned())
            .unwrap_or_else(|| fallback_name(&id.symbol));
        let qualified_name = data
            .and_then(|data| data.qualified_names.first().cloned())
            .unwrap_or_else(|| display_name.clone());
        SymbolView {
            id: id.clone(),
            display_name,
            qualified_name,
            kinds: data
                .map(|data| data.kinds.iter().copied().collect())
                .unwrap_or_default(),
            definition_count: data.map_or(0, |data| data.definitions.len()),
            occurrence_count: data.map_or(0, |data| data.occurrences.len()),
            relationship_count: data.map_or(0, |data| data.relationships.len()),
        }
    }

    fn query_parts<'a>(
        &'a self,
        query: &'a str,
        document: Option<&'a str>,
    ) -> (Option<&'a str>, &'a str) {
        if document.is_some() {
            return (document, query);
        }
        self.documents
            .keys()
            .find_map(|path| {
                query
                    .strip_prefix(path)
                    .and_then(|rest| rest.strip_prefix(':'))
                    .map(|name| (Some(path.as_str()), name))
            })
            .unwrap_or((None, query))
    }

    fn document(&self, path: &str) -> Result<&scip::types::Document, QueryError> {
        self.documents
            .get(path)
            .map(|index| &self.raw.documents[*index])
            .ok_or_else(|| QueryError::UnknownDocument(path.to_owned()))
    }

    fn source_text(&self, document: &scip::types::Document) -> Result<String, QueryError> {
        if !document.text.is_empty() {
            return Ok(document.text.clone());
        }
        let root = self.root.as_ref().ok_or_else(|| {
            QueryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no source root and Document.text is empty",
            ))
        })?;
        safe_source_path(root, &document.relative_path)
            .and_then(|path| fs::read_to_string(path).map_err(QueryError::Io))
    }

    fn snippet(
        &self,
        occurrence: &OccurrenceView,
        before: usize,
        after: usize,
    ) -> Result<SourceSnippet, QueryError> {
        let document = self.document(&occurrence.document)?;
        let text = self.source_text(document)?;
        let range = occurrence
            .range
            .or(occurrence.enclosing_range)
            .ok_or_else(|| QueryError::UnknownSymbol("definition has no range".to_owned()))?;
        let lines: Vec<_> = text.lines().collect();
        let definition_start = usize::try_from(range.start.line)
            .map_err(|_| QueryError::InvalidPosition { line: 0, column: 0 })?;
        if definition_start >= lines.len() {
            return Err(QueryError::InvalidPosition {
                line: definition_start + 1,
                column: range.start.character.max(0) as usize + 1,
            });
        }
        let start = definition_start.saturating_sub(before);
        let requested_end =
            ((range.end.line.max(range.start.line) as usize) + after + 1).min(lines.len());
        let total_lines = requested_end.saturating_sub(start);
        let end = requested_end.min(start + MAX_SNIPPET_LINES);
        Ok(SourceSnippet {
            document: occurrence.document.clone(),
            start_line: start + 1,
            end_line: end,
            total_lines,
            returned_lines: end.saturating_sub(start),
            truncated: end < requested_end,
            text: lines[start..end].join("\n"),
            provenance: if document.text.is_empty() {
                "explicit_source_root_unverified"
            } else {
                "scip_document_text"
            },
        })
    }
}

fn absorb_info(
    symbols: &mut BTreeMap<SymbolId, SymbolData>,
    identity_document: &str,
    info: &SymbolInformation,
    document: Option<String>,
    symbol_information_index: usize,
    external: bool,
) {
    if info.symbol.is_empty() {
        return;
    }
    let id = SymbolId::new(identity_document, &info.symbol);
    let data = symbols.entry(id).or_default();
    if !info.display_name.is_empty() {
        data.display_names.insert(info.display_name.clone());
    }
    data.qualified_names
        .insert(qualified_name(&info.symbol).unwrap_or_else(|| info.display_name.clone()));
    data.kinds.insert(info.kind.value());
    data.documentation.extend(
        info.documentation
            .iter()
            .filter(|text| !text.is_empty())
            .cloned(),
    );
    if let Some(signature) = info.signature_documentation.as_ref()
        && (!signature.language.is_empty() || !signature.text.is_empty())
    {
        data.signatures.insert(SignatureView {
            language: signature.language.clone(),
            text: signature.text.clone(),
        });
    }
    data.information.push(SymbolInformationView {
        document,
        symbol_information_index,
        external,
        symbol: SymbolId::new(identity_document, &info.symbol),
        documentation: info.documentation.clone(),
        kind: info.kind.value(),
        display_name: info.display_name.clone(),
        enclosing_symbol: info.enclosing_symbol.clone(),
        signature: info
            .signature_documentation
            .as_ref()
            .map(|signature| SignatureView {
                language: signature.language.clone(),
                text: signature.text.clone(),
            }),
        relationship_count: info.relationships.len(),
    });
}

fn normalized_range(
    occurrence: &Occurrence,
    enclosing: bool,
) -> (Option<SourceRange>, RangeSource) {
    let typed = if enclosing {
        occurrence
            .typed_enclosing_range
            .as_ref()
            .and_then(|range| match range {
                occurrence::Typed_enclosing_range::SingleLineEnclosingRange(range) => {
                    Some(SourceRange {
                        start: Position {
                            line: range.line,
                            character: range.start_character,
                        },
                        end: Position {
                            line: range.line,
                            character: range.end_character,
                        },
                    })
                }
                occurrence::Typed_enclosing_range::MultiLineEnclosingRange(range) => {
                    Some(SourceRange {
                        start: Position {
                            line: range.start_line,
                            character: range.start_character,
                        },
                        end: Position {
                            line: range.end_line,
                            character: range.end_character,
                        },
                    })
                }
                _ => None,
            })
    } else {
        occurrence
            .typed_range
            .as_ref()
            .and_then(|range| match range {
                occurrence::Typed_range::SingleLineRange(range) => Some(SourceRange {
                    start: Position {
                        line: range.line,
                        character: range.start_character,
                    },
                    end: Position {
                        line: range.line,
                        character: range.end_character,
                    },
                }),
                occurrence::Typed_range::MultiLineRange(range) => Some(SourceRange {
                    start: Position {
                        line: range.start_line,
                        character: range.start_character,
                    },
                    end: Position {
                        line: range.end_line,
                        character: range.end_character,
                    },
                }),
                _ => None,
            })
    };
    if typed.is_some() {
        return (typed, RangeSource::Typed);
    }
    let legacy = if enclosing {
        &occurrence.enclosing_range
    } else {
        &occurrence.range
    };
    match SourceRange::parse(legacy) {
        Some(range) => (Some(range), RangeSource::Legacy),
        None => (None, RangeSource::Missing),
    }
}

fn role_names(roles: i32) -> Vec<&'static str> {
    [
        (SymbolRole::Definition, "definition"),
        (SymbolRole::Import, "import"),
        (SymbolRole::WriteAccess, "write"),
        (SymbolRole::ReadAccess, "read"),
        (SymbolRole::Generated, "generated"),
        (SymbolRole::Test, "test"),
        (SymbolRole::ForwardDefinition, "forward_definition"),
    ]
    .into_iter()
    .filter_map(|(role, name)| (roles & role as i32 != 0).then_some(name))
    .collect()
}

fn is_lexical_scope(id: Option<&SymbolId>, symbols: &BTreeMap<SymbolId, SymbolData>) -> bool {
    let Some(id) = id else {
        return false;
    };
    let kinds = symbols.get(id).map(|data| &data.kinds);
    kinds.is_some_and(|kinds| {
        kinds
            .iter()
            .any(|kind| matches!(*kind, 7 | 9 | 17 | 26 | 29 | 41 | 49))
    }) || scip::symbol::parse_symbol(&id.symbol)
        .ok()
        .and_then(|symbol| symbol.descriptors.last().cloned())
        .and_then(|descriptor| descriptor.suffix.enum_value().ok())
        .is_some_and(|suffix| {
            matches!(
                suffix,
                scip::types::descriptor::Suffix::Namespace
                    | scip::types::descriptor::Suffix::Type
                    | scip::types::descriptor::Suffix::Method
            )
        })
}

fn qualified_name(symbol: &str) -> Option<String> {
    let parsed = scip::symbol::parse_symbol(symbol).ok()?;
    let names: Vec<_> = parsed
        .descriptors
        .into_iter()
        .map(|descriptor| descriptor.name)
        .filter(|name| !name.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join("."))
}

fn fallback_name(symbol: &str) -> String {
    qualified_name(symbol)
        .and_then(|name| name.rsplit('.').next().map(str::to_owned))
        .unwrap_or_else(|| {
            symbol
                .rsplit(['/', '#', '.', '(', ')'])
                .find(|part| !part.is_empty())
                .unwrap_or(symbol)
                .to_owned()
        })
}

fn find_rank(symbol: &SymbolView, needle: &str) -> (u8, String, SymbolId) {
    let display = symbol.display_name.to_lowercase();
    let qualified = symbol.qualified_name.to_lowercase();
    let rank = if display == needle {
        0
    } else if qualified == needle {
        1
    } else if display.starts_with(needle) {
        2
    } else if qualified.starts_with(needle) {
        3
    } else {
        4
    };
    (rank, display, symbol.id.clone())
}

fn resolution(query: &str, mut candidates: Vec<SymbolView>, limit: usize) -> Resolution {
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        0 => Resolution::NotFound {
            query: query.to_owned(),
        },
        1 => Resolution::Found {
            symbol: candidates.pop().expect("one candidate"),
        },
        _ => Resolution::Ambiguous {
            query: query.to_owned(),
            candidates: Page::new(candidates, limit),
        },
    }
}

fn symbol_has_document(index: &QueryIndex, id: &SymbolId, path: &str) -> bool {
    id.document.as_deref() == Some(path)
        || index.symbols.get(id).is_some_and(|data| {
            data.occurrences
                .iter()
                .any(|occurrence| index.occurrences[*occurrence].document == path)
        })
}

fn document_id(path: &str) -> SymbolId {
    SymbolId {
        document: Some(path.to_owned()),
        symbol: "<document>".to_owned(),
    }
}

fn encoded_column(
    text: Option<&str>,
    line: usize,
    column: usize,
    encoding: i32,
) -> Result<i32, QueryError> {
    let byte_column = column - 1;
    let Some(line_text) = text.and_then(|text| text.lines().nth(line - 1)) else {
        return Ok(byte_column as i32);
    };
    if byte_column > line_text.len() || !line_text.is_char_boundary(byte_column) {
        return Err(QueryError::InvalidPosition { line, column });
    }
    let prefix = &line_text[..byte_column];
    Ok(
        match PositionEncoding::from_i32(encoding).unwrap_or_default() {
            PositionEncoding::UTF8CodeUnitOffsetFromLineStart => byte_column as i32,
            PositionEncoding::UTF16CodeUnitOffsetFromLineStart => {
                prefix.encode_utf16().count() as i32
            }
            PositionEncoding::UTF32CodeUnitOffsetFromLineStart => prefix.chars().count() as i32,
            PositionEncoding::UnspecifiedPositionEncoding => byte_column as i32,
        },
    )
}

pub(crate) fn safe_source_path(root: &Path, relative: &str) -> Result<PathBuf, QueryError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(QueryError::UnsafePath(relative.to_owned()));
    }
    let root = root.canonicalize()?;
    let joined = root.join(relative_path);
    let canonical = joined.canonicalize()?;
    if !canonical.starts_with(&root) {
        return Err(QueryError::UnsafePath(relative.to_owned()));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::{Document, SymbolInformation, symbol_information};

    #[test]
    fn ripgrep_byte_columns_convert_to_scip_encodings() {
        let text = "a💚b\n";
        assert_eq!(encoded_column(Some(text), 1, 6, 1).unwrap(), 5);
        assert_eq!(encoded_column(Some(text), 1, 6, 2).unwrap(), 3);
        assert_eq!(encoded_column(Some(text), 1, 6, 3).unwrap(), 2);
        assert!(encoded_column(Some(text), 1, 3, 1).is_err());
    }

    #[test]
    fn ordinary_reference_does_not_inherit_the_target_owner() {
        let parent = "test package example 1.0 Parent#";
        let target = "test package example 1.0 Parent#member.";
        let index = Index {
            documents: vec![Document {
                relative_path: "use.py".into(),
                symbols: vec![
                    SymbolInformation {
                        symbol: parent.into(),
                        display_name: "Parent".into(),
                        kind: symbol_information::Kind::Class.into(),
                        ..Default::default()
                    },
                    SymbolInformation {
                        symbol: target.into(),
                        display_name: "member".into(),
                        enclosing_symbol: parent.into(),
                        kind: symbol_information::Kind::Method.into(),
                        ..Default::default()
                    },
                ],
                occurrences: vec![Occurrence {
                    range: vec![0, 0, 1],
                    symbol: target.into(),
                    symbol_roles: SymbolRole::ReadAccess as i32,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let query = QueryIndex::from_index(index, None).unwrap();
        let occurrence = &query.at("use.py", 1, None, 1).unwrap().items[0];
        assert_eq!(occurrence.owner, Some(document_id("use.py")));
    }

    #[test]
    fn non_utf8_exact_position_requires_source_text() {
        let index = Index {
            documents: vec![Document {
                relative_path: "missing.py".into(),
                position_encoding: PositionEncoding::UTF16CodeUnitOffsetFromLineStart.into(),
                occurrences: vec![Occurrence {
                    range: vec![0, 0, 1],
                    symbol: "local 0".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let query = QueryIndex::from_index(index, None).unwrap();
        assert!(matches!(
            query.at("missing.py", 1, Some(1), 1),
            Err(QueryError::Io(_))
        ));
    }

    #[test]
    fn unspecified_encoding_uses_utf8_columns_without_source_text() {
        let index = Index {
            documents: vec![Document {
                relative_path: "missing.py".into(),
                occurrences: vec![Occurrence {
                    range: vec![0, 0, 1],
                    symbol: "local 0".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let query = QueryIndex::from_index(index, None).unwrap();
        assert_eq!(query.at("missing.py", 1, Some(1), 1).unwrap().returned, 1);
    }

    #[test]
    fn stale_source_range_becomes_a_snippet_failure() {
        let symbol = "example package demo 1.0 pkg/value().";
        let index = Index {
            documents: vec![Document {
                relative_path: "short.py".into(),
                text: "value = 1\n".into(),
                symbols: vec![SymbolInformation {
                    symbol: symbol.into(),
                    display_name: "value".into(),
                    kind: symbol_information::Kind::Function.into(),
                    ..Default::default()
                }],
                occurrences: vec![Occurrence {
                    range: vec![5, 0, 5],
                    symbol: symbol.into(),
                    symbol_roles: SymbolRole::Definition as i32,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let query = QueryIndex::from_index(index, None).unwrap();
        let id = SymbolId {
            document: None,
            symbol: symbol.into(),
        };
        let context = query.context(&id, 2, 2).unwrap();
        assert_eq!(context.snippets.returned, 0);
        assert_eq!(context.snippet_failures.returned, 1);
        assert!(context.snippet_failures.items[0].error.contains("6:1"));
    }
}
