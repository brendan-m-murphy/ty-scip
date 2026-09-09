# ty-scip distribution plan

Status: **Proposed; naming and release channel awaiting final confirmation**
Started: 2026-09-09

## Goal

Make `ty-scip` straightforward to install and operate as an independent,
community-maintained alternative to `scip-python`, without creating a second
implementation or an unnecessary Python API.

## Recommended identity

Retain `ty-scip` consistently for the repository, Cargo package, executable,
PyPI distribution, and SCIP symbol scheme.

- ty supplies the semantic engine; Ruff supplies the shared parser, database,
  project, and text infrastructure. `ruff-scip` would misstate that boundary
  and could imply an official Ruff project.
- Pronounce the name “tee-why skip” and always retain the hyphen. Avoid
  `scip-py`, `pyscip`, and `python-scip`, which invite confusion with SciPy or
  the SCIP optimization/PySCIPOpt ecosystem.
- Use the tagline “A fast SCIP indexer for Python, powered by ty.”
- State prominently that this is an independent project, not affiliated with
  or endorsed by Astral or Sourcegraph.
- Do not install a compatibility executable named `scip-python`; it would
  collide with the Sourcegraph tool and imply the wrong provenance.

As of 2026-09-09, exact registry checks found `ty-scip` unclaimed on PyPI,
crates.io, unscoped npm, and GitHub. Availability is not reserved until the
projects are created. `python-scip-indexer` is the vendor-neutral fallback if
the use of `ty` becomes undesirable.

## Packaging decision

The product remains one Rust executable. Publish that executable to PyPI as
platform wheels using Maturin's `bin` binding. Do not add PyO3, a Python
subprocess wrapper, an import package, runtime dependencies, first-run binary
downloads, telemetry, or an update checker.

This gives Python users the expected tool installation paths without making
Python part of the indexer runtime:

```console
uvx ty-scip index .
uv tool install ty-scip
pipx install ty-scip
pip install ty-scip
```

The Python used by a wheel installer is unrelated to the Python environment
that ty selects for the indexed project. Documentation must keep those two
concepts separate.

Keep `publish = false` in Cargo. The pinned ty/Ruff crates are unpublished Git
dependencies, so crates.io cannot represent the build. Do not publish a PyPI
source distribution for the first release: prototype it separately, but a
source build would still require a recent Rust toolchain, Git/network access,
and a large Ruff checkout. A tagged GitHub checkout is the canonical source.

## Release stages

### A. Compatible preview CLI

Support the common `scip-python` command shape while retaining the current
pre-release shorthand during 0.x:

```console
ty-scip index [PROJECT_PATH] --output index.scip
```

Add only the compatibility options with clear semantics:

- optional `index` subcommand;
- `--output PATH`;
- `--cwd PATH`; and
- `--quiet`.

Keep `--project-name` and `--project-version`. Reject rather than silently
ignore `--environment`, `--project-namespace`, and `--target-only` until their
semantics can be implemented faithfully. Document how `ty.toml` and
`[tool.ty]` replace Pyright configuration.

### B. Binary preview release

Add a minimal `pyproject.toml` with a pinned Maturin build backend,
`bindings = "bin"`, and stripped release binaries. Cargo metadata remains the
single source of the product version.

Smallest credible wheel and GitHub-archive matrix:

- Linux x86-64, manylinux 2.17;
- Linux ARM64, manylinux 2.17;
- macOS x86-64;
- macOS ARM64; and
- Windows x86-64, only after Windows output-replacement tests pass.

Each tagged build must install the wheel in a clean environment, run
`ty-scip --version`, index a fixture twice with identical bytes, decode the
index, and run the supported SCIP consumer gate. Matching GitHub archives must
contain the same binary, `LICENSE`, reviewed third-party notices, and published
SHA-256 checksums.

Publish wheels from a protected GitHub Actions environment using PyPI Trusted
Publishing. Do not store a long-lived PyPI token.

### C. Operational integration

After the wheel release is stable, add only channels with demonstrated users:

- a minimal GHCR image and Sourcegraph executor example when auto-indexing is
  being tested;
- a Homebrew tap after stable tagged releases exist; and
- a scoped npm launcher only if Node-first `scip-python` users require it.

Conda, an install script, musllinux/Alpine, Windows ARM64, FreeBSD, a Python
library API, and automatic updates are deferred.

## Release gates

Before the first binary preview:

1. Fix the full dotted import-module target regression found by differential
   OpenGHG comparison; imported module spans must not collapse to the root
   package.
2. Add the small CLI compatibility surface and migration documentation.
3. Run ordinary CI on Linux, macOS, and Windows; do not claim Windows until
   atomic replacement, URI, and smoke tests pass there.
4. Generate and review a locked third-party notice bundle covering Ruff/ty,
   embedded typeshed data, and transitive binary dependencies.
5. Build and install every wheel, then run deterministic fixture and SCIP
   consumer checks against the installed executable.
6. Run the frozen OpenGHG scale, structural-integrity, differential, and
   `scip-cli` query gates against the release candidate.
7. Tag `v0.1.0`; record the pinned Ruff commit in release notes. Deliberate
   symbol-identity changes require at least a pre-1.0 minor version and a
   changelog warning.

## Files expected to change

- `Cargo.toml`: author/readme metadata; keep the current package and pin model.
- `pyproject.toml`: minimal Maturin binary-wheel configuration.
- `src/main.rs` and `tests/cli.rs`: compatibility command and options.
- `src/scip_emit.rs` and `tests/emission.rs`: Windows-safe replacement policy.
- `.github/workflows/ci.yml`: platform smoke matrix.
- `.github/workflows/release.yml`: tagged wheels, archives, checksums,
  attestations, and Trusted Publishing.
- `README.md` and a migration document: install paths, independence statement,
  configuration differences, and option mapping.
- A generated, reviewed binary notice bundle before artifacts are published.

## Immediate sequence

1. Confirm the `ty-scip` public identity before creating the remote.
2. Fix and regression-test dotted import-module targets.
3. Implement the bounded CLI compatibility surface.
4. Add and locally smoke-test Maturin `bin` packaging.
5. Create `brendan-m-murphy/ty-scip`, push, and exercise hosted platform CI.
6. Build release-candidate wheels and notices; publish only after review.
