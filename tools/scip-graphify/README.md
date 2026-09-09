# scip-graphify

Experimental, indexer-neutral conversion from SCIP protobuf indexes to
Graphify-compatible JSON. This is a separate Cargo package from `ty-scip` and
depends only on SCIP, protobuf, and JSON serialization crates.

```console
cargo run --release --manifest-path tools/scip-graphify/Cargo.toml -- \
  index.scip graph.json
graphify path add_timed_data plan_timed_data_update --graph graph.json
graphify affected plan_timed_data_update --graph graph.json
```

The disposable JSON retains exact SCIP symbols, kinds, occurrence ranges and
roles, enclosing-definition ownership, and relationships. It emits
`references`, `imports`, `implements`, and derived `contains` edges. It never
describes a plain SCIP reference as a runtime call.

The converter accepts compatible indexes from any producer, including
`ty-scip` and `scip-python`. It does not import ty or Ruff, is not installed by
the `ty-scip` Python wheel, and does not create a public `ty_scip` Rust library
API.

See the [spike record](../../docs/research/scip-graphify-spike.md) for measured
results, limitations, and the accuracy gate for any future SQLite query cache.
