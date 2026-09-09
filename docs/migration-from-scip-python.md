# Migrating from scip-python

`ty-scip` accepts the common `scip-python` indexing shape:

```console
ty-scip index . --output index.scip
```

The `index` token is optional, so the original `ty-scip . index.scip`
shorthand remains available during 0.x. Use `./index` to index a project
directory literally named `index`.

The compatible options are:

| `scip-python` command | `ty-scip` command |
| --- | --- |
| `scip-python index PROJECT --output FILE` | `ty-scip index PROJECT --output FILE` |
| `scip-python index --cwd DIR` | `ty-scip index --cwd DIR` |
| `scip-python index --quiet` | `ty-scip index --quiet` |

`--cwd` is the base for both relative project paths and relative output paths.
`--quiet` suppresses indexing samples and the final summary, but errors still
go to stderr and return a non-zero exit status.

`ty-scip` does not silently accept `--environment`, `--project-namespace`, or
`--target-only`. These options fail as unsupported until their behavior can be
implemented faithfully.

## Configuration

`ty-scip` uses ty for project discovery, source selection, import resolution,
and Python-environment selection. Configure those behaviors with `ty.toml` or
the `[tool.ty]` table in `pyproject.toml`. It does not read
`pyrightconfig.json`.

The emitted symbol scheme is `ty-scip`, so indexes are not byte-for-byte or
symbol-for-symbol replacements for existing `scip-python` indexes. Generate a
fresh index when switching indexers.
