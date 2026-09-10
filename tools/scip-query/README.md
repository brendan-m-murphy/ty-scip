# scip-query

`scip-query` is a small static counterpart to ty's Python LSP navigation. It
reads a SCIP index directly and offers bounded, deterministic JSON projections
of the operations a coding agent normally needs after orienting with `rg`.

It does not provide natural-language graph queries, architecture inference,
test ranking, transitive impact analysis, a SQLite cache, or an MCP server.

## Install

```console
cargo install --locked --path tools/scip-query
```

Create a ty index and its optional static IDE results:

```console
ty-scip index . --output index.scip --facts index.tyfacts
```

The sidecar is loaded automatically when it is named beside the index as
`index.tyfacts`; `--facts PATH` selects another path. A fingerprint mismatch is
an error. With the sidecar, definitions, hover, references, call hierarchy, and
type hierarchy are the results returned by ty's own `ty_ide` functions during
indexing. Without it, standard SCIP provides portable fallbacks for every
command except `callers` and `callees`. Version 1 sidecars from earlier builds
are intentionally rejected; regenerate them with the matching `ty-scip`.

## Commands

```text
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] find QUERY [--path PREFIX]
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] at PATH:LINE[:COLUMN]
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] definition SELECTOR
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] hover SELECTOR
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] references SELECTOR [--path PREFIX] [--offset N]
scip-query --index INDEX.scip [--facts INDEX.tyfacts] [--root PATH] [--limit N] members SELECTOR
scip-query --index INDEX.scip --facts INDEX.tyfacts [--limit N] callers SELECTOR
scip-query --index INDEX.scip --facts INDEX.tyfacts [--limit N] callees SELECTOR
scip-query --index INDEX.scip [--limit N] supertypes SELECTOR
scip-query --index INDEX.scip [--limit N] subtypes SELECTOR
```

Locations are one-based and columns use UTF-8 byte offsets, matching
`rg --column`. Selectors may be raw SCIP symbols, qualified names, bare names,
or `path.py:Qualified.name`. Ambiguity is returned as structured JSON rather
than resolved arbitrarily. Workspace discovery and bare-name selection ignore
import-local bindings and return their canonical targets. `references` omits
imports, matching editor reference navigation. `callers` and `callees` group
the exact call-hierarchy items and call-site ranges returned by ty. Hover text
is rendered by ty rather than reconstructed from SCIP documentation. Output is
compact, one-line JSON; pipe it through `jq` when inspecting it manually.

The intended agent loop is deliberately simple:

1. use `rg` to locate relevant terms or dynamic behavior;
2. use `at` or `find` to obtain a semantic symbol;
3. follow `definition`, `references`, `callers`, or `callees`; and
4. inspect source before making a behavioral claim.
