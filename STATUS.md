# LeseLang development status

The [0.2.3 project revision](README.md#leselang) includes the registered GUI
conformance suite for Rust, Node and C#, with [worker phase timings](benchmarks/worker-phases.md)
for the two out-of-process Consumers.

The versioned [development tensor](project/status/catalog.json) is the
authoritative inventory of current capability status. Each coordinate has
an owner, a scoped contract, prerequisites, evidence and an acceptance gate.
Use `summary` for closure counts across the registered contracts and modules.
`next` reports completion of the ordered mainline goals. Its output includes
both populations so an individual milestone can be read in context.
Historical runs retain their dates and platform boundaries in the
[acceptance archive](docs/acceptance/README.md).

Build the independent tool from the repository root. After the initial fetch,
validation and queries work offline.

```sh
cargo fetch --locked --manifest-path tools/leselang-dev/Cargo.toml
sh scripts/check.sh tensor
target/lean/leselang-dev tensor summary
target/lean/leselang-dev tensor tree --architecture runtime
target/lean/leselang-dev tensor list --module leselang-syntax --status stable
target/lean/leselang-dev tensor weakest --priority active
target/lean/leselang-dev tensor blocked
target/lean/leselang-dev tensor inspect runtime/leselang-vm/durable-runtime
target/lean/leselang-dev tensor next --json
```

All commands support `--json` and exact `--architecture`, `--module`,
`--function`, `--status` and `--priority` filters. Use `--root PATH` when
running outside the repository. JSON responses carry schema version 1.
Exit codes are 0 for a valid query, 1 for invalid data or a missing coordinate
and 2 for invalid usage.

Read the [tensor protocol](docs/development-tensor.md) for field and evidence
semantics and the [mainline guide](docs/development-mainline.md) for task selection.
