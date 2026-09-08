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
- Repeated indexing of the same checkout with the same arguments is
  byte-for-byte deterministic.
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

1. Use Ruff's UTF-8 line index as the single source for legacy and typed SCIP
   ranges, including CRLF, lone-CR, Unicode, and enclosing-range tests.
2. Emit trustworthy occurrence roles from syntax context.
3. Add documentation and signatures to `SymbolInformation` where stable.
4. Emit SCIP relationships supported by the semantic evidence.
5. Preserve SCIP 0.8 consumer compatibility alongside SCIP 0.10 typed ranges
   until the benchmark consumer no longer requires it.
6. Add package/distribution identity before emitting external links; never
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

Complete stable lexical identities for named nested callables and classes,
then harden output metadata, file URIs, and atomic replacement. After that,
add syntax-error and unreadable-file resilience before extending package
ownership. The remaining 39 first-party ambiguities stay unresolved until a
typed bulk occurrence API can remove constructor/`__call__` expansion without
a first-target heuristic.

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
- **2026-09-08:** Completed the pinned public ty/Ruff API map. Useful next
  surfaces include semantic tokens, semantic definition/place tables, method
  decorators, docstrings, type hierarchy, dependency ownership, and Ruff's
  line index. Cursor-oriented reference and implementation APIs remain useful
  as targeted oracles, not as per-occurrence bulk indexer APIs. The primary
  upstream opportunity is a stable bulk resolved-occurrence API with exact
  ranges, roles, targets, ownership, aliases, signatures, and documentation.
- **2026-09-08:** Recorded the second OpenGHG agent benchmark at revision
  `b9822d1`. Coverage increased from 428 to 464 SQLite chunks and from 15,293
  to 15,999 mentions, and all 24 cited locations were valid. It did not
  demonstrate an efficiency win over built-in search: total tokens were about
  34% higher, uncached input 6.5% higher, and elapsed time 2% slower. This makes
  `ty-scip` a credible fast structural-navigation backend, but agent efficiency
  is not yet a release claim.
- **2026-09-08:** Reproduced the benchmark's nondeterministic local-symbol
  metadata. Four symbols alternated between equivalent alias or qualified-name
  display strings because hash-map order selected the retained metadata.
  Local groups now choose the shortest, then lexicographically smallest name;
  serialized symbol information is sorted by symbol and definition range.
  Four fresh release-mode OpenGHG runs using identical arguments produced the
  same SHA-256 (`d2fa1291dc941f6cc0c91ff972ec0d260a1e4c75769b15f61b872035bfa1169b`)
  with unchanged occurrence and omission counts.
- **2026-09-08:** Split the implementation into a CLI/reporting entry point, a
  ty/Ruff indexing module, and a DB-free SCIP emission module. This introduced
  no framework or trait boundary. The post-split release build reproduced the
  exact pre-split OpenGHG SHA-256 and counters.
- **2026-09-08:** Moved complete semantic-definition allocation ahead of
  instance-member promotion. Public `scope_ids`, `use_def_map`, and
  `definitions_with_usage` can enumerate unused locals, imports,
  comprehensions, patterns, exception targets, parameters, walruses, type
  parameters, and nested definitions. Promoting member groups on top of this
  inventory avoids two allocation implementations and stabilizes local numbers
  before reference coverage changes.
- **2026-09-08:** Replaced the hand-written newline scanner with Ruff's pinned
  UTF-8 `LineIndex`. One index is reused per document for legacy and SCIP 0.10
  typed occurrence/enclosing ranges. Focused tests cover LF, CRLF, lone CR, and
  multibyte UTF-8 columns; this fixes lone-CR coordinates without changing the
  consumer compatibility policy.
- **2026-09-08:** Preallocated all uniquely representable user-visible semantic
  definitions before reference resolution. OpenGHG definitions increased from
  16,688 to 22,975, while references and resolution counters were unchanged;
  all three missing-symbol skips disappeared. Shared-range definitions such as
  wildcard expansions remain in the semantic grouping table but are not
  serialized as an arbitrary single local symbol.
- **2026-09-08:** Promoted ty-proven receiver attributes in direct undecorated
  methods to durable class-member symbols and canonicalized whole-expression
  targets to the attribute token. On OpenGHG references increased from 40,738
  to 41,235 and cross-file-local skips fell from 634 to 3, with unresolved and
  ambiguous counts unchanged. Decorated methods remain conservatively omitted.
- **2026-09-08:** Hardened the dependency-free CLI: help/version work, indexing
  defaults to `index.scip`, normal stdout is quiet, and errors use an actionable
  `ty-scip:` prefix. Tests now decode SCIP edges instead of treating debug edge
  output as a contract.
- **2026-09-08:** Added stable first-party package identity without claiming
  `scip-python` symbol compatibility. Explicit name/version flags override
  static PEP 621 metadata, which overrides an empty deterministic fallback.
  `src/` remains a filesystem layout rather than a module descriptor. OpenGHG
  resolution counts and lint results were unchanged.
- **2026-09-08:** Mapped trustworthy richer occurrence roles. Ruff syntax
  contexts can supply read, write, delete, import, and direct augmented
  read/write roles without workspace rescans; semantic definitions overlay the
  definition bit. String annotations, pytest fixture strings, `__slots__`, and
  speculative generated/test roles remain deferred. `scip-python` itself emits
  only definition/read, so this is a useful extension rather than a parity
  blocker.
- **2026-09-09:** Emitted definition, import, read, write, and augmented
  read/write roles from Ruff syntax plus ty's semantic import classification.
  Exact duplicate range/symbol occurrences merge deterministically while
  preserving definition enclosing metadata; different symbols never merge.
  Dedicated decoded tests cover imports, locals, members, exception targets,
  and match bindings. OpenGHG semantic counts remained unchanged and repeated
  output stayed byte-identical.
- **2026-09-09:** Added raw ty docstrings and deterministic Ruff-AST signatures
  to symbol information. Modules, classes, functions, documented assignments,
  nested functions, overloads, and getter/setter co-definitions have focused
  decoded coverage. Hover parsing, inferred/property signatures, PEP 257/reST
  rendering, parameter docs, and stub-to-source fallbacks remain explicit API
  gaps. OpenGHG semantic counts were unchanged in the canonical environment.
- **2026-09-09:** Emitted direct first-party class bases as SCIP implementation
  relationships using one public `type_hierarchy_supertypes` query per class.
  Focused coverage includes cross-module bases, sorted multiple inheritance,
  and exclusion of `object`, metaclasses, and external targets. OpenGHG emitted
  57 relationships with unchanged occurrence counters, byte-identical repeated
  output, and about 0.05 seconds steady-state overhead. Bulk
  `goto_type_definition` queries were prototyped and removed after causing an
  unacceptable scale-test slowdown; reverse `goto_implementation` scans are
  likewise not suitable for bulk override discovery. Both tested SCIP linters
  intermittently report missing cross-document relationship targets on the
  larger index even though every target exists in document symbol information;
  the error set changes across runs of identical bytes, while the focused
  cross-file fixture lints cleanly. No invalid external-symbol workaround was
  added for that consumer defect.
