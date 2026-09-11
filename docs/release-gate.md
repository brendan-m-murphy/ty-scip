# Frozen OpenGHG release gate

The release gate checks a candidate `ty-scip` binary against OpenGHG commit
`a4d352f5972e84774ad36f238af9fe2eec2c4d2e` and the patched `scip-python`
reference at PR 226 commit `468008597e371ed4abac73ea2a14a08bbd16c7d1`.
It clones both repositories itself; builds the reference; requires two
byte-identical candidate indexes; checks decoded SCIP integrity; permits only
the six reviewed property/inherited-receiver target divergences; and exercises
a fresh `scip-cli` cache with the issue 1714 search, member, reference, and
dependency probes. Timings and coverage counts are recorded, but they are not
release thresholds.

## Prerequisites

- [uv](https://docs.astral.sh/uv/);
- Git;
- Node.js and npm;
- network access to GitHub and the npm registry; and
- the candidate `ty-scip` executable to test.

From the repository root, run:

```console
uv run --no-project --with scip-cli==2.7.0 python scripts/openghg_release_gate.py --ty-scip ./target/release/ty-scip
```

The ephemeral `scip-cli` environment supplies and caches its pinned SCIP 0.8.1
binary inside the gate's isolated home.

The temporary checkout is removed after success or failure. Pass `--keep` to
retain it, or `--work-dir PATH` to select and retain a new or empty directory.
A successful retained run includes `metrics.json`, `differential.json`, both
SCIP indexes, their JSON representations, and the fresh consumer database/cache.
An earlier failure leaves only the artifacts produced before that step.

SCIP 0.8.1 lint status is recorded rather than used as a hard gate because that
consumer can report valid cross-document relationship targets inconsistently.
The decoded checks still require every definition, relationship target, and
standard-library occurrence to have the corresponding symbol information.

The complete deterministic differential is locked by SHA-256. Any semantic
change therefore stops the gate until `differential.json` has been reviewed and
the expected digest is deliberately updated.
