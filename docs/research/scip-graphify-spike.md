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

## Product directions after the query benchmark

The benchmark supports three complementary interfaces over the same lossless
SCIP authority. They should not be collapsed into one large default response.

1. **Interactive, LSP-like navigation.** Agents use `find` or `at`, then make
   small `context`, `refs`, `members`, and `path` requests as each answer
   suggests the next symbol. This is the default for understanding behavior.
   `rg` remains useful for strings, configuration, registrations, and dynamic
   relationships that static semantics cannot resolve.
2. **Grouped change-surface projections.** Questions such as “which tests guard
   this symbol?” benefit from bounded transitive traversal, but results should
   be grouped by file and ranked by shortest evidence path. Keep direct-only
   (`--depth 0`) and occurrence-level output available for audit and detail.
   Grouped rows should identify a representative test function and behavioral
   snippet, classify the evidence as direct, downstream-contract, or incidental,
   and expose the terminal symbol plus bounded follow-up selectors. Agents should
   issue one important selector per command and query each material terminal
   implementation layer before synthesizing a change surface.
3. **Architecture orientation.** A derived SQLite graph can answer higher-level
   questions without reducing the underlying protobuf. This is where the useful
   part of Graphify's promise belongs: identify likely public entry points,
   delegating wrappers versus implementation-heavy symbols, module layers,
   shared subsystems, inheritance families, fan-in/fan-out hotspots, and common
   paths from public APIs to persistence or computation boundaries.

Architecture reports must state how each label was derived. Candidate evidence
includes exported/re-exported symbols and package facades for public surface;
resolved callee positions and short forwarding paths for wrapper/work
distinctions; and normalized module dependencies, ownership, inheritance,
centrality, and communities for recurring patterns. Naming conventions and
graph measures are ranking signals, not semantic facts. Dynamic registrations,
plugin loading, configuration keys, decorators, and string-dispatched APIs need
source/`rg` evidence alongside SCIP.

The next downstream experiments, in order, are:

1. measure whether relevance-labelled grouped output and terminal-layer
   follow-up recover contract tests that agents previously saw but ignored;
2. document the interactive query loop in the agent skill and measure whether
   agents use fewer broad reads while retaining accuracy;
3. add lossless module-level aggregates and an evidence-returning `explain`
   query for public-surface and wrapper/worker hypotheses;
4. join synchronized ty callee-position facts when available, and compare the
   resulting paths with conservative callable-reference traversal; and
5. evaluate architectural summaries against hand-mapped Python subsystems,
   including false-positive and dynamic-edge audits.

Do not add embeddings or a natural-language query layer until these bounded,
deterministic queries prove insufficient. The model can translate a question
into explicit commands while the tool returns inspectable evidence.

### Initial file-grouped result

On OpenGHG, grouping `BaseStore.assign_data` test evidence reduced 52
occurrence/role rows (about 18 KB) to 17 file summaries (about 6.5 KB) without
losing the depth-3 path to `tests/store/test_datasource.py`. In one frozen
semantic agent replicate it reduced specialized SCIP invocations from 20 to 8,
but final quality remained 17/18 because the agent still did not select that
contract test for inspection. Keep grouping as output hygiene; do not treat it
as a substitute for following the architectural stages interactively and
querying their tests directly.

The first audit-driven refinement now makes that interaction explicit. Grouped
rows prefer test-function evidence over imports, include a behavioral snippet
when `--root` is supplied, label the relationship, and return both the terminal
symbol and follow-up selectors. The benchmark prompt must still require agents
to act on those leads; output structure alone cannot ensure source inspection.
