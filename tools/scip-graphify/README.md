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
roles, enclosing-definition ownership, relationships, documentation, and
signature text. Documentation entries are deduplicated and deterministically
sorted in `scip_documentation`; the first canonical entry is exposed as
`scip_description`, and their combined text is exposed as Graphify's searchable
`rationale` attribute. Signature language and text are likewise retained in
the sorted `scip_signatures` list. It emits `references`, `imports`,
`implements`, and derived `contains` edges. It never describes a plain SCIP
reference as a runtime call.

The converter accepts compatible indexes from any producer, including
`ty-scip` and `scip-python`. It does not import ty or Ruff, is not installed by
the `ty-scip` Python wheel, and does not create a public `ty_scip` Rust library
API.

When symbol-kind metadata is absent or conflicting, standard global SCIP
namespace, type, and method descriptors still identify lexical owners. Local
symbols without kind metadata remain conservatively file-owned because their
descriptor contains no kind evidence.

See the [spike record](../../docs/research/scip-graphify-spike.md) for measured
results, limitations, and the accuracy gate for any future SQLite query cache.
