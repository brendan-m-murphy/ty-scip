# ty-scip public-readiness plan

Status: **in progress**
Started: 2026-09-08

## Goal

Make `ty-scip` a reliable public alternative to `scip-python`: correct enough
for code-navigation agents, explicit about unsupported dynamic Python, and
maintainable as a small external consumer of pinned public ty/Ruff APIs.

Parity means equivalent useful SCIP relationships, not identical symbols or
protobuf bytes. A missing link is preferable to a false link.

## Baseline

The MVP is complete. On frozen OpenGHG it emits 281 documents, 16,688
definition occurrences and 40,738 references in 1.93 seconds with a release
build. It reports 9,754 unresolved identifier queries, 39 ambiguous queries,
12,212 external queries, and 634 safely skipped internal edges. Both SCIP
0.8.1 and 0.10 lint pass, and the isolated `scip-cli` #1714 gate passes.

## Public-readiness gates

- No known false first-party links in focused fixtures or the frozen OpenGHG
  audit.
- Stable global symbols for modules, classes, callables, parameters, class and
  instance members, aliases, and re-exports that SCIP consumers query.
- Deterministic local symbols for repeated definitions, nested scopes,
  comprehensions, lambdas, patterns, exception targets, and imports.
- Accurate definition/read/write/import roles wherever Ruff syntax identifies
  them without guessing.
- Useful symbol information: kind, display name, enclosing range, documentation
  and signature when public ty APIs expose them reliably.
- Relationships needed for class navigation, especially implementation and
  type-definition edges when representable in SCIP.
- Project discovery and Python-environment behavior documented and covered for
  packages, `src/` layouts, namespace packages, `.pyi`, excludes, syntax errors,
  and missing environments.
- CLI errors are actionable; partial project failures do not silently produce a
  misleading successful index.
- Release artifacts have a README, license/notice, reproducible dependency pin,
  CI, and a documented update procedure for the Ruff revision.
- Golden SCIP-level tests and a repeatable differential evaluation against
  `scip-python` cover representative static Python constructs.
- The actual supported `scip-cli` search, members, refs, and deps gate passes on
  a freshly converted database.

## Workstreams

### 1. Semantic coverage

1. Promote proven `self.attr`/`cls.attr` definitions to durable class-member
   symbols, including definitions in more than one method.
2. Replace editor-navigation expansion with typed public semantic resolution
   for names, attributes, imports, and keyword arguments where exact target
   matching proves it is safer.
3. Enumerate and classify unresolved queries by syntax context before adding
   fallbacks. Do not heuristically link unknown attributes.
4. Cover aliases, wildcard imports, re-exports, properties, overloads,
   decorators, protocols, TypedDict fields, and dataclass-generated behavior.
5. Explore public ty APIs for inherited members, implementations, type
   definitions, docstrings, signatures, and diagnostics. Record any missing
   stable API as a narrow Astral request.

### 2. SCIP fidelity

1. Emit trustworthy occurrence roles from syntax context.
2. Add documentation and signatures to `SymbolInformation` where stable.
3. Emit SCIP relationships supported by the semantic evidence.
4. Preserve SCIP 0.8 consumer compatibility alongside SCIP 0.10 typed ranges
   until the benchmark consumer no longer requires it.
5. Add package/distribution identity before emitting external links; never
   pretend stdlib or site-packages belong to the indexed project.

### 3. Tests and evaluation

1. Decode generated SCIP protobufs in tests and assert exact ranges, symbols,
   roles, relationships, and symbol information rather than byte substrings.
2. Port the smallest representative fixtures from `scip-python`; add explicit
   false-link regressions for every normalization policy.
3. Build a deterministic semantic comparison report against `scip-python`
   without requiring identical scheme-specific symbol strings.
4. Keep frozen OpenGHG as the scale and `scip-cli` acceptance gate, recording
   release time, index size, definitions, references, omissions, and query
   results at each material milestone.

### 4. Public project surface

1. Replace the positional spike interface with a small documented CLI only
   where real options require it; preserve the simple default command.
2. Add README, license/third-party notice, contribution/update notes, and
   supported/unsupported feature documentation.
3. Add CI for formatting, Clippy, tests, release build, and SCIP lint on the
   minimum practical platform matrix.
4. Decide binary distribution only after source builds are reproducible; avoid
   a release pipeline before there is a release.

## Working rules

- Keep all direct Astral crates at exactly one commit.
- Prefer a public upstream API over copied analyzer logic or a fork.
- Never serialize Salsa, scope, place, or node IDs.
- Every new normalization rule needs a positive fixture and a false-link
  fixture.
- Record benchmark commands and artifacts well enough to distinguish debug,
  release, stale-cache, and isolated-cache results.
- Commit each independently verified milestone.

## Current next step

Complete the feature/API parity map, then implement class-owned instance-member
symbols as the next evidenced navigation gap. The remaining 39 first-party
ambiguities stay unresolved until typed occurrence resolution can remove
constructor/`__call__` expansion without a first-target heuristic.

## Progress log

- **2026-09-08:** Promoted the completed MVP into this public-readiness plan.
  Started parallel audits of `scip-python`, the pinned public ty/Ruff surface,
  test parity, architecture, and release requirements.
- **2026-09-08:** Completed the first parity audit. The critical gaps are
  project/package identity, proven class-owned instance members, external
  distribution ownership, documentation/signatures, inheritance/override
  relationships, import/config/error fixtures, and the public CLI/release
  surface. Diagnostics, notebooks, and roles beyond definition/read are not
  `scip-python` parity blockers.
- **2026-09-08:** Added decoded-protobuf test support using the already-locked
  `protobuf` crate as a dev dependency. Keyword overload and reaching-definition
  tests now assert exact SCIP symbols, ranges, and roles instead of relying on
  protobuf byte substrings.
