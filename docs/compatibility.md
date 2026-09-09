# Compatibility and limitations

`ty-scip` aims for useful semantic parity with `scip-python`, not identical
protobuf bytes or symbol strings. Its safety rule is simple: omit a link when
the available ty evidence does not identify one durable symbol.

## SCIP consumers

Every occurrence contains both forms of its range:

- SCIP 0.10 typed `single_line_range` or `multi_line_range`; and
- the deprecated integer `range` and `enclosing_range` fields consumed by
  older tooling.

Focused indexes pass the SCIP 0.10 linter and have been exercised through the
SCIP 0.8.1-based conversion path in `scip-cli` 2.7.0. The acceptance gate uses
a fresh conversion database and checks non-zero chunks and mentions plus
search, members, references, and dependency queries. Both linter versions
intermittently misreport valid cross-document relationship targets on the
larger OpenGHG index; decoded integrity checks prove those targets have symbol
information and definition occurrences, and repeated lint runs over identical
bytes report different subsets. Supporting consumers older than the tested
conversion path is not currently a goal.

Ranges use UTF-8 byte offsets and Ruff's universal-newline line index. Tests
cover LF, CRLF, lone CR, and multibyte text.

## Feature matrix

This table describes implemented behavior, not the broader public-readiness
plan.

| Area | `ty-scip` now | `scip-python` comparison |
| --- | --- | --- |
| Project files | ty-selected first-party `.py` and `.pyi` files, including namespace packages and configured excludes; file symlinks are selected but directory symlinks are not traversed | Supported through Pyright's project model, including directory symlinks |
| Global definitions | Modules, classes, functions, methods, constructors, variables, constants, properties, fields, and type parameters | Broadly supported |
| Callable parameters | Stable global symbols beneath named callables | Supported, including Pyright's deeper callable model |
| Local definitions | Deterministic semantic bindings, including unused and repeated definitions | Broadly supported, including nested constructs |
| First-party references | Unambiguous names, attributes, imports, re-exports, keyword arguments, and analyzer-confirmed names inside quoted annotations | Broadly supported |
| Overloads and repeated definitions | Co-definitions normalize when they resolve to one durable symbol | Supported through Pyright declaration identity |
| Instance attributes | Promoted when ty proves a receiver attribute in a direct method with normal inferred receiver semantics, including identity-preserving decorators; inherited reads resolve | Broader handling through Pyright's class/member model |
| Imports and aliases | Relative, aliased, dotted, submodule, and `__init__.py` re-export targets when unambiguous; dynamic/wildcard edge cases are not claimed | More mature import, alias, and re-export handling |
| Package identity | One first-party PEP 621 or explicit name/version for the whole index, plus configured-version `python-stdlib` identities | First-party, standard-library, and installed-distribution identities |
| External links | Proven runtime standard-library symbols are emitted; typing-only and installed third-party targets are counted and omitted | Standard-library and third-party symbols can be emitted |
| Occurrence roles | Definition, import, read, write, and augmented read/write | Definition and read |
| Symbol information | Kind including semantically verified properties and modern type aliases, display name, raw ty docstrings, source-faithful callable/class/annotated-assignment/type-alias signatures, and enclosing ranges | Emits rendered documentation and signatures through Pyright internals |
| Inheritance and overrides | Direct first-party class bases are emitted as implementation relationships; inherited reads resolve | Emits class/implementation relationships, including richer internal cases |
| Symbol scheme | `ty-scip`; intentionally not symbol-compatible | `scip-python` scheme |
| SCIP ranges | SCIP 0.10 typed plus legacy fields | Legacy-consumer compatible |
| Diagnostics and notebooks | Parser diagnostic counts are reported; recovered syntax is indexed; no SCIP diagnostics/notebooks | No SCIP diagnostics/notebooks; not a parity blocker |

`ty-scip` preallocates user-visible semantic definitions before reference
resolution. This includes locals, imports, parameters, comprehensions,
patterns, exception targets, walrus bindings, type parameters, and nested
definitions when ty exposes a uniquely representable binding. Shared source
ranges that map to several distinct bindings are not assigned an arbitrary
symbol.

## Identity and omission rules

Global first-party symbols use the `ty-scip` scheme, Python as the package
manager, one project package identity, importable module components, lexical
containers, and the SCIP descriptor kind. `src/` is a filesystem layout, not a
symbol descriptor.

Package identity precedence is:

1. `--project-name` and `--project-version`;
2. static PEP 621 `[project].name` and `[project].version`; then
3. an empty value.

Dynamic versions are not executed or imported. A module that ty proves belongs
to the runtime standard library uses `python-stdlib` and ty's configured Python
major/minor version; typing-only modules such as `_typeshed` are excluded.
Multi-distribution monorepos, editable-install ownership, and installed
distribution ownership are not implemented.

Named nested functions and classes use stable lexical global symbols. Other
function-local symbols are allocated deterministically in source order. When
several declarations represent one semantic binding, their local symbol uses
one deterministic display name. Tool metadata intentionally omits host command
arguments, so a repeated run is byte-identical when the checkout, project
configuration, and package identity are identical, even if the output path
changes.

Resolution outcomes mean:

- **unresolved**: ty returned no declaration target for the queried syntax;
- **ambiguous**: distinct first-party or mixed targets remained after safe
  binding and overload normalization;
- **external**: every target was outside the indexed first-party file set; and
- **skipped**: a target existed but could not be serialized safely, such as a
  document-local symbol referenced from another file.

These are occurrence-query counters, not diagnostics and not a claim that all
Python syntax has been enumerated. They include legitimate external names and
syntax that is not a semantic reference, so lower totals are not automatically
better.

Known conservative omissions include:

- installed third-party and typing-only external links;
- receiver attributes in class/static/property methods or decorators that
  replace the function, where a `self`/`cls` assumption would be false;
- genuinely dynamic attributes and imports;
- string references that are not annotations, such as pytest fixture names and
  `__slots__` entries;
- method-override, type-definition, external-base, and dynamic-base
  relationships;
- rendered/normalized docs, inferred and property-specific signatures,
  parameter docs, stub-to-source doc fallback, diagnostics, and call hierarchy;
  and
- exact `scip-python` symbol compatibility.

Both source and stub documents are indexed when ty selects a parallel `.py` and
`.pyi`. Corresponding declarations share a durable symbol identity, while call
signatures and keyword targets follow the selected stub contract. Stub-only
modules are also supported. A future source-navigation policy may add explicit
stub-to-implementation relationships; it will not replace declaration identity
by default.

## Evaluated ty/Ruff API surface

The current adapter already uses ty project discovery, Ruff parsing and line
indexes, ty document symbols, declaration navigation, semantic scopes,
definition/place tables, and proven instance-member places.

The following public APIs at the pinned revision were evaluated rather than
blindly reimplemented:

| API or evidence | Useful capability | Current decision |
| --- | --- | --- |
| `semantic_tokens` | Precise token ranges and modifiers, including some string annotations | Analyzer-confirmed token ranges inside string literals are passed to declaration resolution; tokens alone are never treated as durable targets |
| `find_references` and document highlights | Reference and local read/write evidence | Keep as fixture/oracle tools; Ruff syntax contexts provide production roles without reverse workspace scans |
| `type_hierarchy_supertypes` | Direct base-class information | Used once per indexed class to emit first-party implementation relationships |
| `goto_implementation` | Implementation targets | Not used: its reverse, per-cursor project scan is unsuitable for bulk override indexing |
| `goto_type_definition` | Type targets for expressions | Explored, then deferred: per-definition cursor queries caused an unacceptable OpenGHG slowdown |
| `hover` | Rendered signatures and documentation | Not parsed: raw public definition docstrings and source-faithful Ruff-AST declaration slices are emitted instead |
| property/accessor and method-decorator inference plus semantic place tables | Distinguish properties and prove class-owned members | Property/accessor, receiver-semantics, and member-place evidence are used; transformed callables remain conservative |
| module/dependency ownership | Basis for external package identities | Public search-path and configured-version evidence is used for the runtime standard library; installed distribution ownership/version still needs a complete public API |

The most valuable upstream addition would be a stable bulk resolved-occurrence
API carrying exact ranges, roles, canonical targets, aliases, ownership,
signatures, and documentation. Until then, `ty-scip` keeps ty/Ruff coupling in
one module and avoids copying analyzer logic or maintaining a fork. The
concrete remaining seams and conservative interim decisions are listed in
[candidate upstream ty APIs](upstream-ty-api-requests.md).

## Platform and release limits

The current source build is the distribution mechanism. Index files are
written through an exclusively created sibling temporary file, flushed, and
atomically renamed over the destination; failed writes remove their temporary
file. Project roots use standards-based percent-encoded file URIs. An
unreadable or undecodable selected source file is an actionable indexing error,
and the output path is neither created nor replaced; parser errors in readable
files instead use Ruff's recovered tree and are reported in the summary.

The Cargo package is
marked `publish = false`, the ty/Ruff dependencies are pinned Git crates, and
there are no release binaries. macOS is exercised locally; Windows behavior
is not claimed until file-URI and replacement-rename behavior are tested in
CI. A project license, locked transitive third-party notice inventory, and an
automated SCIP consumer gate are required before calling the repository
release-ready.

The pinned crates have no external API-stability guarantee. Pin updates are
deliberate compatibility work, not routine dependency bumps.

## Validation and pin updates

For ordinary changes:

1. Run `cargo fmt --all --check`.
2. Run `cargo clippy --locked --all-targets -- -D warnings`.
3. Run `cargo test --locked` and `cargo build --release --locked`.
4. Generate the same fixture twice with identical arguments and compare the
   bytes.
5. Lint with SCIP 0.10 and perform an isolated `scip-cli` 2.7.0 conversion and
   query gate when emission changes.

For a Ruff pin update:

1. Change every direct Ruff/ty dependency to the same full commit in
   `Cargo.toml`; mixed revisions are unsupported.
2. Regenerate and commit `Cargo.lock`.
3. Resolve public API changes without copying private analyzer logic.
4. Run the ordinary gate above, including decoded-SCIP positive and false-link
   fixtures.
5. Re-run the frozen OpenGHG scale check and record time, index size,
   definition/reference/omission counters, deterministic bytes, lint, and the
   isolated search/members/references/dependencies results.
6. Compare the semantic edges with `scip-python`; do not require identical
   scheme-specific symbol strings.
