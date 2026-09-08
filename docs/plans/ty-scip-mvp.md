# ty-scip MVP plan

Status: **in progress**  
Updated: 2026-09-08

## Goal

Prove that a small Rust binary can produce a useful SCIP index for first-party
Python code by consuming ty/Ruff crates at one pinned Git revision, without
forking Ruff or querying an LSP server.

The MVP is deliberately narrower than `scip-python`: repository-local
definitions and references first; distribution-aware external navigation and
drop-in symbol compatibility only after the core approach works.

## Current state

- Planning and source/API reconnaissance are complete.
- The workspace started as an empty Git repository.
- Phase 0 is complete. Phase 1 now includes stable callable-parameter symbols
  and a non-fatal policy for unsupported cross-file local targets.
- Rust 1.98.1 was installed after the initial environment check. The Codex app
  shell has not refreshed its `PATH`, so commands currently use
  `/Users/bm13805/.cargo/bin/cargo` explicitly.
- The external consumer compiles against the pinned Ruff revision without a
  patch or checkout, and the four semantic smoke cases pass.
- Phase 0 writes a deterministic SCIP 0.10 index. The official `scip lint`
  command accepts the smoke index, and `scip snapshot --strict=false` shows the
  expected cross-file and local links.
- The frozen OpenGHG checkout now produces a complete 4.1 MB index in 3.4-7.5
  seconds across observed runs. The official SCIP 0.10 linter accepts it. The
  run skipped and reported 306 cross-file targets whose identities were only
  document-local.
- The issue #1714 tool-only search/member/reference/dependency gate passes with
  the actual `scip-cli` 2.7.0 binary from a separate OpenGHG root and cache.

## Verified decisions

- Implement in Rust. ty's useful semantic APIs are Rust crates; Python would
  add a binding or IPC layer without helping the indexer.
- Pin every Ruff/ty Git dependency to the same commit and commit `Cargo.lock`.
- Do not fork Ruff for the MVP.
- Use ty's project/configuration and file discovery rather than duplicate it.
- Traverse the parsed Ruff AST once per file and resolve occurrences with ty.
- Use the official Rust SCIP bindings and SCIP symbol utilities.
- Prefer correct omissions over false links: unresolved or multiply resolved
  references are counted and skipped.
- Unsupported cross-file document-local targets are counted and skipped rather
  than aborting an otherwise useful repository index.
- Keep ty/Ruff coupling in one module so a future supported Astral API can
  replace it.
- Start with a `ty-scip` symbol scheme. Claim `scip-python` compatibility only
  after differential tests establish it.
- Emit SCIP 0.10 typed ranges plus equivalent deprecated range fields. This is
  the smallest compatibility seam for the benchmark's `scip-cli` 2.7.0, whose
  conversion path pins SCIP 0.8.1; broader legacy behavior remains out of scope.

The inspected Ruff commit is
`12132db2885084f4b87aafc5908e336e7b5e8fbd`. At that revision the necessary
building blocks are public: `ProjectDatabase`, project-file enumeration,
`SemanticModel`, semantic indices and definitions, and name/attribute/import
resolution. They are still internal, unpublished crates and have no stability
promise.

## Minimal architecture

```text
ty project discovery
  -> ProjectDatabase and first-party files
  -> Ruff parsed AST / source-order visitor
  -> ty definition resolution
  -> deterministic SymbolKey normalization
  -> SCIP documents, occurrences, and symbols
  -> index.scip
```

Keep the implementation to one binary and, if separation is useful, no more
than these modules:

- `main.rs`: arguments, project setup, reporting, and output.
- `ty_index.rs`: all direct ty/Ruff interaction and AST traversal.
- `scip_emit.rs`: stable identities, ordering, ranges, and SCIP construction.

No plugin framework, analyzer abstraction, daemon, persistent cache, parallel
pipeline, Python extension, or custom protobuf layer.

## MVP scope

Include:

- First-party `.py` and `.pyi` files selected by ty.
- Module, class, function/method, variable, parameter, and local definitions.
- Name, attribute, import, and re-export references where resolution is
  unambiguous.
- Definition/import/read/write roles when the AST provides them reliably.
- Deterministic document paths, UTF-8 ranges, occurrences, and symbol
  information.
- A summary of files, definitions, references, unresolved occurrences,
  ambiguous occurrences, and skipped external targets.

Defer:

- Third-party and standard-library cross-repository linking.
- Notebooks, implementations, inheritance graphs, call hierarchy, diagnostics,
  and rich signatures.
- Incremental/watch mode, caching, parallelism, and streaming serialization.
- Multi-distribution monorepos and exact `scip-python` symbol compatibility.

## Symbol identity rules

- Never serialize Salsa IDs, scope IDs, definition IDs, or byte offsets as
  global symbol identities.
- Build global identities from project identity, module name, lexical
  containers, name, and SCIP descriptor kind.
- Allocate function-local SCIP symbols deterministically in source order.
- Normalize multiple reaching definitions of one semantic binding to one
  symbol key.
- Emit an edge only when all resolved candidates normalize to one symbol.
- Skip external targets until package/distribution ownership is trustworthy.
- Give callable parameters global identities beneath their callable when that
  callable itself has a stable global symbol.

This is the main design risk. Occurrence enumeration is already exposed well
enough for the spike.

## Implementation phases

### Phase 0: feasibility spike

1. Confirm the Rust toolchain and build a minimal external Cargo project
   against the exact Ruff pin.
2. Construct `ProjectDatabase` using ty's discovery and enumerate project
   files.
3. Parse and walk a tiny fixture once in source order.
4. Resolve four end-to-end cases: a cross-file function, an aliased import, a
   method/attribute, and a shadowed local.
5. Write a minimal deterministic `index.scip` and run SCIP validation/query
   tooling.

### Phase 1: definitions and deterministic output

1. Implement symbol keys for modules, containers, members, and locals.
2. Emit definitions and one `SymbolInformation` per global symbol.
3. Convert Ruff byte ranges to SCIP positions, including Unicode tests.
4. Sort all serialized data explicitly and prove repeatable output.

### Phase 2: references

1. Add names, attributes, imports, and re-exports.
2. Add reliable occurrence roles.
3. Count and skip ambiguous, unresolved, and external targets.
4. Add string-annotation traversal only if the existing public submodel API
   makes it a small change.

### Phase 3: evaluation

1. Run golden fixtures covering imports, re-exports, attributes, locals,
   shadowing, conditional definitions, overloads, `.pyi`, `src/` layouts,
   Unicode, and recoverable syntax errors.
2. Compare semantic target agreement with `scip-python`; do not require byte
   equality.
3. Record cold runtime, peak memory, index size, and resolution counters on a
   representative repository.
4. Decide whether to continue, revise the API seam, or prepare a narrowly
   evidenced Astral request.

## Acceptance and stop gates

Phase 0 passes only if:

- `cargo build --locked` works without modifying Ruff.
- The four spike cases resolve correctly through public APIs.
- SCIP validation accepts the output and existing query tooling can find the
  expected cross-file definitions/references.
- Two unchanged runs produce byte-identical output.

The MVP passes only if:

- All hand-authored in-scope golden expectations pass with no known false
  links.
- Unsupported cases are omitted and visible in counters.
- The implementation remains a small direct adapter rather than a second
  analyzer.

Stop and seek upstream support if semantic resolution requires an LSP query per
token, copied private analyzer logic, a Ruff fork, or broad visibility changes.
One to three small missing public APIs are grounds for a focused upstream
request, not a fork.

## Dependencies

Direct Phase 0 dependencies, minimized by the external compile check:

- Ruff/ty Git crates at one exact revision: `ty_project`, `ty_ide`,
  `ty_module_resolver`, `ruff_db`, `ruff_python_ast`, and `ruff_text_size`.
- Official `scip` Rust crate pinned to 0.10.0.
- Avoid convenience crates until stdlib code becomes materially worse. A tiny
  CLI can initially use `std::env`; add an argument parser only when the CLI has
  enough options to justify it.

The repository will pin a compatible Rust toolchain once the external build has
been proven. Ruff's inspected workspace uses Rust 2024 edition and pins a recent
toolchain, so the exact compiler requirement is part of Phase 0 evidence.

## Possible upstream outcome

After the prototype proves the use case, ask Astral for a small,
analyzer-neutral occurrence API or stability/documentation for the existing
public pieces—not SCIP-specific behavior. A useful primitive would expose an
occurrence range, role, resolved definition candidates, enclosing definition,
durable lexical path, and module/distribution ownership.

## Progress log

- **2026-09-08:** Recorded the initial architecture and gated MVP scope.
- **2026-09-08:** Source reconnaissance found a plausible public no-fork path
  at Ruff commit `12132db2885084f4b87aafc5908e336e7b5e8fbd`.
- **2026-09-08:** Selected Rust and a first-party repository-local MVP; deferred
  external package identity and `scip-python` compatibility.
- **2026-09-08:** Rust 1.98.1 became available; pinned the project toolchain to
  it.
- **2026-09-08:** Added a minimal Cargo binary pinned to the inspected Ruff
  revision and a two-file smoke project covering the four Phase 0 cases.
  Deliberately deferred SCIP serialization until semantic enumeration compiles.
- **2026-09-08:** `cargo check` passed against the exact Ruff pin. The first run
  showed that `visit_identifier` omits ordinary name expressions, so traversal
  was corrected once at `SourceOrderVisitor::enter_node` rather than adding
  case-specific scans.
- **2026-09-08:** Added one integration smoke test. `cargo test --locked`
  resolves all four Phase 0 semantic cases successfully.
- **2026-09-08:** Added the pinned official SCIP 0.10.0 binding and emitted a
  1.5 KiB smoke index with typed UTF-8 ranges and experimental `ty-scip`
  symbols.
- **2026-09-08:** The official SCIP 0.10.0 CLI accepts the index with
  `scip lint`; the integration test checks byte determinism directly.
- **2026-09-08:** Review corrected project-root handling, replaced filesystem
  module paths with ty's importable module names, preserved external candidates
  during ambiguity checks, added module definition occurrences, and changed
  repository-local package identity to the empty SCIP package.
- **2026-09-08:** Expanded the smoke fixture to a configured `src/` layout and
  invoke the indexer from the `src` subdirectory, covering both discovered-root
  and import-name behavior. Initially kept SCIP output typed-only.
- **2026-09-08:** `cargo fmt --check`, strict Clippy, `cargo test --locked`,
  `scip lint`, and a non-strict SCIP snapshot pass.
- **2026-09-08:** Final review found no remaining Phase 0 blocker. Deferred the
  policy for non-importable project files to Phase 1 instead of speculating.
- **2026-09-08:** Added a two-module keyword-argument regression. Parameters of
  globally identified functions and methods now receive stable SCIP parameter
  symbols, so `time_offset(period=...)` links across modules without a local
  identity. The regression and official SCIP lint pass.
- **2026-09-08:** Changed the unsupported cross-file-local case from a fatal
  error to a counted omission. This keeps the no-false-links invariant while
  allowing useful partial coverage on real repositories.
- **2026-09-08:** Indexed frozen OpenGHG commit
  `a4d352f5972e84774ad36f238af9fe2eec2c4d2e`. Observed runs take 3.4-7.5
  seconds; the 3.7 MB output passes official SCIP 0.10 lint and reports 306
  skipped unsupported cross-file-local targets.
- **2026-09-08:** The first issue #1714 query gate revealed that symbol metadata
  alone is insufficient: class and function definitions also need explicit
  definition occurrences with their full enclosing ranges. Added those ranges
  from ty's `SymbolInfo::full_range`.
- **2026-09-08:** Re-indexed OpenGHG in 3.4 seconds and passed the tool-only
  issue #1714 gate. Search finds `ModelScenario` at `_scenario.py:96` and
  `fp_x_flux_time_resolved_numba` at `_fp_x_flux.py:483`; members lists the
  class methods; legacy references include `_scenario.py:57` and `:1394`;
  Numba references are limited to the package export and its tests; and method
  dependencies include `_modelled_obs.py:216 fp_x_flux_time_resolved` but not
  the Numba implementation.
- **2026-09-08:** Review found and fixed one parameter-scope edge case: lambda
  parameters nested in a global function no longer inherit that function's
  durable parameter namespace. The keyword fixture now guards this behavior.
- **2026-09-08:** Reproduced the real benchmark consumer failure with
  `scip-cli` 2.7.0 and its pinned SCIP 0.8.1 converter: 281 documents survived,
  but chunks, mentions, and definition ranges were all zero. Added deprecated
  occurrence and enclosing ranges alongside the typed SCIP 0.10 ranges, with a
  focused unit check for both encodings.
- **2026-09-08:** The isolated SCIP 0.8.1 conversion now contains 281 documents,
  411 chunks, 15,191 mentions, and 6,351 definition ranges. The resulting 4.1
  MB index still passes SCIP 0.10 lint, and the complete search, members,
  references, and file-dependencies acceptance gate passes.

## Deliberate follow-ups

- Decide the policy for a project file for which ty cannot derive an importable
  module name after observing a real case.
- Normalize overloaded-call keyword targets before the ambiguity cardinality
  check when multiple declarations all denote the same durable parameter.
