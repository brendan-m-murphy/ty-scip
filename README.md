# ty-scip

`ty-scip` is an experimental SCIP indexer for Python. It turns ty's Python
project model and semantic navigation results into a deterministic index that
is tested with SCIP 0.10 and the SCIP 0.8-based `scip-cli` 2.7 conversion path.

The indexer is written in Rust because ty and Ruff expose the required parser,
project, and semantic APIs as Rust crates. `ty-scip` consumes those crates
directly at one pinned Ruff commit: it is a small adapter, not a Ruff fork and
not an LSP client.

## Status

This is pre-release software. It already indexes useful first-party structure,
but it is not yet a drop-in replacement for `scip-python`. The pinned ty/Ruff
crates are unpublished internal crates without an API-stability promise, and
the `ty-scip` symbol scheme may still change.

The project is MIT licensed. There is no supported binary or crates.io
distribution yet; the pinned ty/Ruff crates are unpublished, so build the
current GitHub checkout to evaluate it.

## Build

The crate declares Rust 1.96 and is currently exercised with Rust 1.98.1.
Building needs Git access to fetch the pinned Ruff revision.

```console
cargo build --release --locked
./target/release/ty-scip --help
```

To install the current checkout on your `PATH`:

```console
cargo install --locked --path .
```

## Use

```console
ty-scip [OPTIONS] [PROJECT_PATH] [OUTPUT.scip]
```

With no arguments, `ty-scip` indexes the current directory and writes
`index.scip` there. If only `PROJECT_PATH` is supplied, the output is still
written as `index.scip` in the caller's current directory.

```console
# Current project -> ./index.scip
ty-scip

# Another project discovery path -> explicit output
ty-scip ../project ./project.scip

# Override the package identity recorded in global symbols
ty-scip --project-name example --project-version 1.2.3 ../project
```

Options:

- `--project-name NAME`: override the SCIP package name.
- `--project-version VERSION`: override the SCIP package version.
- `-h`, `--help`: print help.
- `-V`, `--version`: print the version.

Normal output is quiet on stdout. A summary of indexed definitions,
references, unresolved and ambiguous queries, external targets, safely skipped
links, and parser diagnostics is written to stderr. Set
`TY_SCIP_SAMPLE_LIMIT=N` to include up to `N` deterministic examples from each
unresolved and ambiguous category.

The positional project path is where ty starts configuration discovery; an
ancestor `ty.toml` or `pyproject.toml` may determine the actual project root.
Project discovery, source selection, import resolution, and Python-environment
behavior come from ty. Configure them with `ty.toml` or `[tool.ty]` in
`pyproject.toml`; Pyright configuration is not read. File symlinks are selected,
but ty does not traverse symlinked directories. Package name and version come
from the command-line overrides first, then static PEP 621 `[project]`
metadata, then an empty deterministic fallback.

## What it indexes

The current index includes:

- first-party `.py` and `.pyi` files selected by ty;
- modules, classes, callables, parameters, type parameters, properties,
  fields, variables, imports, and function-local bindings;
- unambiguous first-party name, attribute, import, re-export, and keyword
  references;
- analyzer-confirmed names inside quoted annotations, without scanning
  ordinary string contents;
- normalization of overloads and repeated definitions that denote one binding;
- generated dataclass/NamedTuple/TypedDict constructor fields and TypedDict
  string-key reads when ty resolves them to declared fields;
- stable lexical symbols for named nested functions and classes while anonymous
  and ordinary function-local bindings remain document-local;
- class-member identities for instance attributes that ty proves belong to a
  direct method whose inferred callable semantics preserve normal receiver
  behavior, including inherited reads and safe decorated methods;
- direct first-party class-base implementation relationships;
- SCIP definition, import, read, write, and augmented read/write roles, symbol
  kinds including semantically verified properties, display names, docstrings,
  source-faithful callable/class/annotated-assignment/type-alias signatures,
  and enclosing ranges; and
- both SCIP 0.10 typed ranges and equivalent legacy range fields.

Missing semantic evidence is an omission, not a guessed link. Proven runtime
standard-library targets use a `python-stdlib` package identity with ty's
configured Python major/minor version. Typing-only and installed third-party
targets remain counted omissions. Distinct multi-target results remain
ambiguous, document-local identities are not linked across files, and
transformed-method receiver attributes are skipped.

See [compatibility and limitations](docs/compatibility.md) for the detailed
feature matrix and the ty APIs evaluated for future work.

## Compatibility and evidence

The dual range encoding passes SCIP 0.10 lint on the focused fixtures and
supports the SCIP 0.8-based conversion path used by `scip-cli` 2.7.0.
Compatibility is tested at the query layer because protobuf validity alone
does not prove that converted mentions survive. Both tested linter versions
intermittently misreport valid cross-document relationship targets on the
larger OpenGHG index; every reported target has symbol information and a
definition occurrence, and the error set changes between runs of identical
bytes.

On the frozen 281-document OpenGHG checkout, repeated release-mode runs with
the same project produced byte-identical indexes, including when written to
different output paths. The latest index converted to 542 chunks and 20,303
mentions and passed the isolated `scip-cli` search, code, members, references,
dependencies, and reverse-dependencies gate. Index replacement uses an
exclusively created sibling temporary file followed by an atomic rename. Two
planning tasks produced accurate scopes and 24/24 valid cited locations, but
the benchmarked arm did not beat built-in search: it used about 34% more total
tokens, 6.5% more uncached input, and 2% more elapsed time. These results
support `ty-scip` as a fast structural-navigation backend, not an agent
efficiency claim.

## Development

Run the smallest complete local gate before submitting a change:

```console
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Semantic changes need a decoded-SCIP regression that proves both the desired
link and the relevant false-link case. Before changing the Ruff pin, follow
the update checklist in [compatibility and limitations](docs/compatibility.md).
The remaining analyzer seams are recorded as narrow
[candidate upstream ty APIs](docs/upstream-ty-api-requests.md).

## License

Copyright 2026 Brendan Murphy. Released under the [MIT License](LICENSE).
