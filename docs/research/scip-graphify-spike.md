# SCIP-to-Graphify spike

Date: 2026-09-09
Status: **concluded; implementation removed**

## Result

[PR #1](https://github.com/brendan-m-murphy/ty-scip/pull/1) proved that a
standard SCIP index can be projected into Graphify without importing ty or
Ruff and without discarding occurrence evidence. On the frozen 281-document
OpenGHG index, conversion took about 0.8 seconds and produced 65 MiB of JSON;
Graphify loaded it in 1.4 seconds with about 409 MB peak memory.

The one-replicate [issue #8](https://github.com/brendan-m-murphy/ty-scip/issues/8)
pilot matched `rg` at 38/38 expected facts and was 14.7% faster, but used 16%
more total tokens. That did not establish a material navigation advantage.
[PR #10](https://github.com/brendan-m-murphy/ty-scip/pull/10) explored a smaller
offline query layer and was also closed because no concrete use case justified
maintaining it beside `rg` and a live ty language server.

## Decision

The converter and its CI were removed. The portable `.scip` file remains the
canonical artifact; normal source search and live ty queries remain the default
agent workflow. Git history preserves the working prototype. Reconsider a graph
or sidecar layer only for a concrete consumer and after an independent benchmark
shows a useful advantage.
