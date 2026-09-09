# SCIP-to-Graphify spike

Date: 2026-09-09
Status: **working prototype; no default query backend proposed**

## Question

Can Graphify consume ty's precise Python semantics without reducing the SCIP
index through `scip-cli`'s chunk-oriented SQLite schema?

The prototype establishes this boundary:

```text
ty-scip or scip-python -> index.scip -> scip-graphify -> graph.json
```

`scip-graphify` is an independent package under `tools/`. It reads only SCIP
data and must never become a second ty/Ruff indexer. The canonical artifact is
still `index.scip`; JSON is a disposable interoperability view.

## Result

The evidence-preserving converter retains document and symbol identities,
document-qualified local symbols, every occurrence range and role,
smallest-enclosing-definition ownership, descriptor hierarchy, explicit SCIP
relationships, documentation, and signature text. It emits references from the
enclosing dependent symbol to the target. Standard SCIP implementation
relationships become Graphify `implements` edges; they are not assumed to mean
class inheritance. A plain reference is never described as a runtime call. The
`.scip` file remains the complete source of truth.

On the frozen 281-document OpenGHG index used for the final prototype,
conversion took about 0.7 seconds and produced 21,537 nodes and 96,123
occurrence-level edges in about 63 MiB of JSON. Graphify 0.9.55 loaded the result for `path`,
`affected`, `explain`, and bounded `query`. The exact reference from
`Datasource.add_timed_data` to `plan_timed_data_update` retained its owning
method and source range. Broad natural-language traversal was noisier than the
precise path and affected commands.

The size is mostly representation overhead: repeated occurrences between the
same symbols become parallel edges. The final prototype contained 59,424
distinct directed endpoint pairs.

## Possible query-store follow-up

If JSON size or query latency blocks a real workflow, generate a disposable,
normalized SQLite cache from `.scip` with tables for documents, symbols,
definitions, occurrences, relationships, and distinct graph edges. Exact
queries must join back to occurrence evidence; path and affected queries can
traverse the distinct edge view with recursive SQL.

Do not reuse `scip-cli`'s current schema as the data model. It discards exact
occurrences, relationships, documentation, signatures, many variable symbols,
and usually external symbols. Merely storing the data in SQLite and expanding
every occurrence back into JSON also does not address the size problem.

Before a SQLite path can replace the direct projection, require:

1. identical document and symbol identities;
2. identical definition ownership and distinct graph edges;
3. reconstruction of every occurrence, range, role, and relationship;
4. matching directed `path` and reverse `affected` results;
5. distinct local identities across documents;
6. correct source/stub co-definitions and incomplete metadata handling; and
7. no `calls` edge without independent producer evidence.

Run the same checks against current `ty-scip` and `scip-python` indexes. Useful
real-project probes include `Datasource.add_timed_data`, class implementation
relationships, `ModelScenario`, and `fp_x_flux_time_resolved_numba`.

## Boundary

All ty/Ruff extraction belongs in `ty-scip`. Facts with an exact standard SCIP
representation—including diagnostics, documentation and signatures,
distribution identity, and implementation relationships—belong in `.scip`.
The downstream tool owns only SCIP-to-graph mapping, serialization, caching,
and traversal. An optional synchronized producer sidecar is reserved for a
demonstrated useful observation SCIP cannot express. The first candidate is
syntactic `CALLEE_POSITION`, not graph policy or a claimed runtime call.
