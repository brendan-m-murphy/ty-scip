# scip-query

`scip-query` is a small, lossless command-line navigator for SCIP indexes. It
decodes the protobuf directly and builds only temporary lookup tables in memory:
the `.scip` file remains the authority for documents, symbols, occurrences,
roles, relationships, signatures, and documentation.

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
scip-query --index INDEX.scip [--root PATH] [--limit N] refs SELECTOR [--incoming|--outgoing|--both]
scip-query --index INDEX.scip [--root PATH] [--limit N] members SELECTOR
scip-query --index INDEX.scip [--root PATH] [--limit N] path SOURCE TARGET [--max-depth N]
scip-query --index INDEX.scip [--root PATH] [--limit N] affected SELECTOR [--max-depth N]
```

Locations are one-based; columns are UTF-8 byte offsets, matching `rg
--column`, and are converted to the index's declared position encoding. A
selector may be a raw SCIP symbol or a
path-qualified name such as `openghg/store/_flux.py:Flux.transform_data`.
Unqualified names are convenient for discovery, but the tool never chooses an
arbitrary match: an ambiguous selector returns a structured non-success result
with candidates that can be used to refine the next request. Exact selectors
that are ambiguous or absent emit JSON and exit with status 2.

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
  (the default).
- `members` returns definition occurrences owned by SCIP enclosing-symbol or
  lexical-range evidence; metadata-only children have no definition to return.
- `path` finds a bounded path through occurrence-backed reference and explicit
  SCIP relationship edges.
- `affected` traverses that graph's incoming edges to report a conservative
  change surface, retaining the predecessor edge and its evidence for each
  result. Neither command implies runtime calls.

For broad textual orientation, use `rg`, then hand an exact hit to SCIP rather
than asking a natural-language graph query:

```console
rg -n 'transform_data' openghg tests
scip-query --index index.scip --root . at openghg/store/_flux.py:103
scip-query --index index.scip --root . refs \
  openghg/store/_flux.py:Flux.transform_data --incoming
```

## Semantic limits

A SCIP read/reference occurrence is not necessarily a runtime call. This tool
therefore reports occurrence-backed references and explicit SCIP relationships;
it does not relabel references as calls. Producer-specific facts emitted by
`ty-scip` can be joined later when their format and need are demonstrated.

The initial tool intentionally has no SQLite cache, natural-language query
layer, embedding index, or MCP server. Those can be added independently if
profiling or agent trials show that direct protobuf queries and JSON output are
insufficient.

Source snippets read through `--root` are marked unverified: the tool constrains
reads to that root but does not yet prove that its files are the exact revision
used to build the index.
