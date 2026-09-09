# SCIP graph-query plan

Status: **prototype; SQLite backend deferred behind an accuracy gate**
Started: 2026-09-09

## Goal

Expose ty's precise Python semantics through bounded graph queries without
making a large JSON graph the source of truth or repeating `scip-cli`'s lossy
conversion. The canonical artifact remains `index.scip`.

```text
ty + Ruff
  -> index.scip
      -> direct Graphify JSON (prototype and parity oracle)
      -> normalized SQLite cache
          -> exact symbol and occurrence queries
          -> compact Graphify-style graph view
```

The graph is for orientation, paths, and impact candidates. Exact answers must
remain traceable to SCIP occurrences, ranges, roles, symbol information, and
relationships.

## Prototype

`scip-graphify` decodes a SCIP protobuf directly and writes deterministic
Graphify-compatible JSON. It does not pass through `scip-cli`'s chunk-oriented
SQLite conversion.

The projection preserves:

- document and global symbol identities;
- document-qualified identities for SCIP local symbols;
- exact occurrence ranges and definition/import/read/write roles;
- ownership by the smallest enclosing definition;
- explicit descriptor hierarchy and SCIP relationships; and
- provenance distinguishing indexed relationships from derived containment.

References point from the enclosing dependent symbol to their target.
Definition containment and class inheritance are represented separately. A
plain SCIP reference is never described as a runtime call because SCIP does
not carry call-site evidence.

On the frozen 281-document OpenGHG index used for the spike, conversion took
about 0.8 seconds and produced 21,528 nodes and 98,933 edges in about 65 MB of
JSON. Graphify `path`, `affected`, `explain`, and bounded `query` commands
loaded the result. Precise traversal retained the reference from
`Datasource.add_timed_data` to `plan_timed_data_update`, including its owning
method and source range. Broad natural-language graph queries were noticeably
noisier than precise traversal.

The JSON size is a representation cost, not an indexer defect. Multiple exact
occurrences between the same pair of symbols become parallel edges. The spike
had only about 60,500 distinct directed endpoint pairs, so a normalized query
store can avoid repeating node and edge attributes while retaining every item
of evidence.

## Lossless SQLite cache

A future SQLite database should be generated from `.scip` and remain a
disposable cache. It must not use the existing `scip-cli` schema as its data
model: that schema drops exact occurrences, relationships, documentation,
signatures, many variable symbols, and usually external symbols.

The smallest useful normalized model is:

- `documents`: one row per SCIP document;
- `symbols`: global and document-qualified local identities plus symbol
  information;
- `definitions`: every definition range and enclosing range;
- `occurrences`: every exact target, range, role, and owning definition;
- `relationships`: every explicit SCIP relationship and flag; and
- `graph_edges`: distinct owner, target, and relation tuples, with evidence
  counts and provenance.

Exact-reference queries join `graph_edges` back to `occurrences`. Path and
affected queries operate over distinct directed edges, preferably through
recursive SQL rather than loading the full graph. Documentation and signatures
remain available for symbol details without being copied into every graph
result.

An aggregated JSON export may remain useful for Graphify interoperability. It
must retain edge multiplicity as a count or weight where graph algorithms use
it. Merely storing data in SQLite and expanding every occurrence back into JSON
does not solve the size problem.

## Accuracy gate

The direct protobuf projection is the initial oracle. A SQLite implementation
must prove, on focused fixtures and a frozen real project:

1. identical document and symbol identities;
2. identical definition ownership, including typed-only ranges;
3. identical distinct `(source, target, relation)` graph edges;
4. reconstruction of the complete occurrence/range/role evidence multiset;
5. preservation of all explicit SCIP relationships and provenance;
6. matching directed `path` and reverse `affected` results for recorded probes;
7. separation of equal local-symbol IDs from different documents;
8. correct source/stub co-definitions and metadata-only parent symbols; and
9. no inferred `calls` edge without independent syntax evidence.

The OpenGHG probes should include storage references around
`Datasource.add_timed_data`, class inheritance, `ModelScenario`, and
`fp_x_flux_time_resolved_numba`. Run the same conversion checks against a
current `scip-python` index so the consumer remains indexer-neutral.

## Additional ty/Ruff evidence

Producer improvements should enrich `.scip` before downstream projection where
the SCIP model can represent them. The most valuable upstream ty APIs are:

- bulk resolved occurrences with exact ranges, roles, canonical targets, and
  explicit unresolved/ambiguous/external outcomes;
- project-walk diagnostics needed to make a complete-index claim;
- installed-distribution ownership and version for safe third-party symbols;
  and
- upward overridden-member lookup for method implementation relationships.

Ruff syntax can also classify whether a resolved occurrence is in callee
position. Because standard SCIP roles do not establish runtime calls, any
`CALLS` relation needs explicit, separately identified syntax evidence rather
than reinterpretation of an ordinary reference.

Current `ty-scip` already emits richer occurrence roles, enclosing ranges,
documentation, signatures, and direct class-base relationships. A normalized
cache should store those fields even when its compact graph view omits them.

## Delivery sequence

1. Keep the direct `scip-graphify` converter as an experimental compatibility
   tool and accuracy oracle.
2. Re-run the frozen-project projection whenever SCIP emission changes and
   retain focused Graphify traversal tests.
3. Prototype the normalized schema only when JSON size or query latency blocks
   a real workflow.
4. Require the accuracy gate before making SQLite the default query path.
5. Add compact or lazy graph queries; keep exact evidence available on demand.
6. Adopt new public ty/Ruff evidence independently as it becomes available.

## Non-goals

- Replacing `index.scip` with SQLite or JSON.
- Treating a semantic reference graph as a proven runtime call graph.
- Guessing ambiguous, dynamic, or cross-document-local links.
- Adding a database dependency before measurements justify it.
- Replacing source inspection or text search for behavioral and dynamic facts.
