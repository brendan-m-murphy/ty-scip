# scip-query

`scip-query` is a small, lossless command-line navigator for SCIP indexes. It
can decode the protobuf directly or materialize a normalized SQLite cache. The
`.scip` file remains the authority for documents, symbols, occurrences, roles,
relationships, signatures, and documentation; the cache retains raw protobuf
records alongside query columns instead of applying `scip-cli`'s pruning.

Install it from the repository checkout:

```console
cargo install --locked --path tools/scip-query
```

```console
cargo run --manifest-path tools/scip-query/Cargo.toml -- \
  --index index.scip --root /path/to/project find ModelScenario

cargo run --manifest-path tools/scip-query/Cargo.toml -- \
  --index index.scip --root /path/to/project context \
  openghg/analyse/_scenario.py:ModelScenario.calc_modelled_obs
```

Output is deterministic JSON on standard output. Diagnostics and errors go to
standard error, so output can be piped directly to `jq`. `--limit` bounds result
lists (the default is 50), except that a found `path` is returned whole and is
bounded by `--max-depth`. Traversal commands accept `--max-depth` (or its short
alias, `--depth`). The index may instead be supplied as the first positional
argument for convenient one-off use.

## Commands

```text
scip-query --index INDEX.scip [--root PATH] [--limit N] find QUERY [--path PATH]
scip-query --index INDEX.scip [--root PATH] [--limit N] at PATH:LINE[:COLUMN]
scip-query --index INDEX.scip [--root PATH] [--limit N] context SELECTOR
scip-query --index INDEX.scip [--root PATH] [--limit N] refs SELECTOR [--incoming|--outgoing|--both] [--path PREFIX] [--compact] [--offset N]
scip-query --index INDEX.scip [--root PATH] [--limit N] members SELECTOR
scip-query --index INDEX.scip [--root PATH] [--limit N] path SOURCE TARGET [--max-depth N]
scip-query --index INDEX.scip [--root PATH] [--limit N] affected SELECTOR [--max-depth N]
scip-query --index INDEX.scip build-db DATABASE
scip-query sql-refs DATABASE SELECTOR [--incoming|--outgoing|--both] [--path PREFIX] [--offset N] [--limit N]
scip-query sql-tests DATABASE SELECTOR [--path PREFIX] [--depth N] [--offset N] [--limit N]
scip-query sql-stats DATABASE
```

Locations are one-based; columns are UTF-8 byte offsets, matching `rg
--column`, and are converted to the index's declared position encoding. A
selector may be a raw SCIP symbol or a
path-qualified name such as `openghg/store/_flux.py:Flux.transform_data`.
Unqualified names are convenient for discovery, but the tool never chooses an
arbitrary match: an ambiguous selector returns a structured non-success result
with candidates that can be used to refine the next request. Exact selectors
that are ambiguous or absent emit JSON and exit with status 2. The common
`Class#method` shorthand is accepted as an alias for `Class.method`; unresolved
selectors include bounded suggestions.

- `find` searches symbol names and SCIP metadata.
- `at` resolves an `rg` location to semantic occurrences. A line-only
  location returns every occurrence spanning that line; adding a column selects
  occurrences containing that exact position.
- `context` returns a resolved symbol, its unmerged SCIP metadata, definitions,
  relationships, occurrences, and bounded source snippets when document text or
  an explicit `--root` is available. Snippet read failures are returned
  explicitly. Index-supplied project roots are not trusted as read authority.
- `refs` returns exact occurrence-backed incoming references, outgoing
  references found inside a definition, explicit SCIP relationships, or both
  (the default). `--path tests/` restricts occurrence evidence by path prefix,
  `--compact` returns only agent-facing symbol/location/role evidence, and
  `--offset` pages through a bounded result without changing the lossless
  default representation.
- `members` returns definition occurrences owned by SCIP enclosing-symbol or
  lexical-range evidence; metadata-only children have no definition to return.
- `path` finds a bounded path through occurrence-backed reference and explicit
  SCIP relationship edges.
- `affected` traverses that graph's incoming edges to report a conservative
  change surface, retaining the predecessor edge and its evidence for each
  result. Neither command implies runtime calls.
- `build-db` creates a new normalized SQLite cache and refuses to overwrite an
  existing file. It preserves occurrence multiplicity and raw occurrence,
  symbol-information, and relationship protobuf records.
- `sql-refs` resolves symbols and queries the cache without decoding the full
  index. It groups repeated occurrences with the same source, target, document,
  roles, and provenance, reporting their count and first location. The direct
  commands remain available for unmerged records and richer context.
- `sql-tests` returns bounded test references to a symbol and follows internal
  callable references up to `--depth` (default 4). Each result includes its
  depth and evidence path. Class projections include directly owned members and
  SCIP implementation/type-definition subtypes; method projections include the
  owning class. Its default path is `tests/`; use `--path` for another test tree.
  Until a synchronized ty callee-position sidecar exists, downstream paths are
  callable-reference paths rather than claims about runtime calls.
- `sql-stats` reports cache row counts for parity checks.

SQL queries collapse document-local import bindings onto the global symbol when
SCIP emits both occurrences at the same source range. A bare imported name such
as `BaseStore` therefore resolves without hiding genuine ambiguity between
distinct global definitions. Ambiguous results contain qualified names,
definition paths, kinds, and copy-paste-safe canonical symbols.

For broad textual orientation, use `rg`, then hand an exact hit to SCIP rather
than asking a natural-language graph query:

```console
rg -n 'transform_data' openghg tests
scip-query --index index.scip --root . at openghg/store/_flux.py:103
scip-query --index index.scip --root . refs \
  openghg/store/_flux.py:Flux.transform_data --incoming --compact --limit 20

# Find direct test evidence without dumping production references.
scip-query --index index.scip --root . refs \
  openghg.store.base._base.BaseStore.assign_data --incoming \
  --path tests/ --compact --limit 20
```

## Semantic limits

A SCIP read/reference occurrence is not necessarily a runtime call. This tool
therefore reports occurrence-backed references and explicit SCIP relationships;
it does not relabel references as calls. Producer-specific facts emitted by
`ty-scip` can be joined later when their format and need are demonstrated.

The tool intentionally has no natural-language query layer, embedding index, or
MCP server. The SQLite cache is an optional performance layer rather than a
replacement interchange format.

Source snippets read through `--root` are marked unverified: the tool constrains
reads to that root but does not yet prove that its files are the exact revision
used to build the index.
