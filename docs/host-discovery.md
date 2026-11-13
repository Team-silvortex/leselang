# Host Catalog Discovery

Editors and agents can inspect a host's operations before constructing source.
The shared catalog description records named parameters, accepted scalar or
native types, result types and required capability labels. It describes a host
profile, with no source file, VM session or native resource connection involved.
Discovery does not grant capabilities or attest that an application, window or
control is currently available.

The [catalog agent walkthrough](agent-workflow.md) demonstrates the next steps:
read an actual catalog, validate an AppKit/TypeScript form or Calculator
contract, generate typed LeseLang source, execute it once and verify its typed
result against native control state. The same agent executable uses declared
semantic tasks when available and known profile strategies for legacy catalogs.
Catalog signatures describe accepted values; task meaning, control selection
and postconditions must be specified explicitly rather than inferred from types.

Hosts may additionally publish [operation semantics and task verification](host-semantics.md)
through `CatalogDescription::to_json_with_semantics`. This optional, separately
versioned extension describes finite call/guard steps and report predicates.
The base Rust structs and `to_json()` output remain unchanged. The reference
agent uses valid declarations without requiring a new profile branch, while
catalogs lacking the extension retain their existing behavior.

## Inspect A Reference Host

The standalone scalar CLI emits its actual registered `std.print` contract:

```sh
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- describe
```

After building, use `target/lean/leselang describe`. Standard output contains
one JSON object followed by LF, with no prose or extra records. The optional
`--diagnostic-format text|json` affects failure records on standard error;
the catalog on standard output is always JSON. `describe` accepts no file,
stdin source, `--allow`, `--fuel`, `--entry` or `--text` options.

The native reference hosts expose the same description schema offline:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --describe
cargo run --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean -- --describe --controls
cargo run --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean -- --describe --widgets --wait-states
cargo run --manifest-path hosts/tauri/Cargo.toml --no-default-features --locked --offline --profile lean -- --describe
cargo run --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host --locked --offline --profile lean -- --describe
```

These modes do not create AppKit controls or a Tauri WebView, connect to a target application,
query Accessibility permission, enumerate windows or execute an Effect. The AX
description mode selects only a catalog profile; application paths, window
selectors, native grants and execution options are not accepted with it.
`--discover` remains a separate, live inventory operation within an explicitly
selected native scope.

| Host invocation | `profile` | Operations |
| --- | --- | --- |
| `leselang describe` | `leselang.scalar` | 1 |
| AppKit `--describe` | `appkit` | 5 |
| AX `--describe` | `macos_ax.textedit` | 3 |
| AX `--describe --calculator` | `macos_ax.calculator` | 3 |
| AX `--describe --controls` | `macos_ax.controls` | 6 |
| AX `--describe --widgets` | `macos_ax.widgets` | 13 |
| AX `--describe --widgets --wait-values` | `macos_ax.widgets.wait_values` | 14 |
| AX `--describe --widgets --wait-states` | `macos_ax.widgets.wait_states` | 17 |
| Tauri `--describe` | `tauri.typescript` | 5 |
| Node driver `--describe` | `javascript.registered` | 5 |

The table describes the current reference profiles. Consumers should read the
actual operation list instead of inferring capabilities from its length. Wait
operations still require the existing explicit host clock and stepping policy;
describing a wait operation does not install a timer or scheduler.
The Node driver's description does not launch Node or enumerate an application's
registered objects. Its operation signatures describe the generic bridge; an
application task and its postconditions remain separate from that metadata.

## Version 1 JSON Contract

The scalar CLI's output has this content, expanded for readability:

```json
{
  "schema_version": 1,
  "protocol_version": 1,
  "catalog_version": 1,
  "profile": "leselang.scalar",
  "operations": [
    {
      "name": "std.print",
      "parameters": [
        {
          "name": "message",
          "required": true,
          "types": [{ "kind": "scalar", "name": "string" }]
        }
      ],
      "result": { "kind": "scalar", "name": "none" },
      "required_capability": "stdio"
    }
  ],
  "native_types": []
}
```

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`, identifying this metadata presentation schema. |
| `protocol_version` | Integer `1`, identifying the existing local host protocol. |
| `catalog_version` | A nonzero host operation-catalog version. It is independent of the other version fields. |
| `profile` | The host's stable label for this particular catalog and native type projection. |
| `operations` | Operation declarations in registration order. |
| `native_types` | Declared native helper type aliases and their exported scalar fields, in declaration order. |

Each operation has an exact `name`, declaration-ordered `parameters`, one
`result` type and its original `required_capability` label. A parameter has its
exact `name`, a Boolean `required` flag and a `types` array. Type alternatives
are explicit accepted alternatives, with no coercion or default. An empty array
means **deny all supplied values**, not "any type". Such a parameter may still
be omitted when `required` is false. Optionality is separate from accepting the
`none` scalar type.

A type description is either `{"kind":"scalar","name":"integer"}` or
`{"kind":"native","name":"object"}`. The six scalar names are `integer`,
`boolean`, `string`, `none`, `optional_string` and `string_list`. Scalar
embedding projects its `ScalarTypeSet` alternatives in that canonical order.
Native descriptions retain the explicit alternative order provided by the host.
Every native reference must name exactly one entry in `native_types`.

A native type entry has `name` and `fields`. Each field has its literal `name`
and a `scalar_type` string from the six scalar names. An opaque `object` can
have no fields, while a `snapshot` exposes the exact fields supported by that
profile. The meaning of a native alias is local to the profile: AppKit's
snapshot `value` is a string, while AX controls' snapshot `value` is an
`optional_string`. Equal alias spellings do not establish a common native ABI,
interchangeable values or object identity.

The record reports type-level signatures. Operation-specific literal choices,
numeric ranges, current control roles or `actions`, enabled state and live
permissions remain subject to the host's existing checks. A matching type alone
does not guarantee that a value or native action will be accepted at runtime.

Array order preserves the host's declarations. JSON object member order and
whitespace are not a parsing contract. Consumers should check `schema_version`,
tolerate unknown fields and handle an unsupported type kind or version
explicitly. Do not silently turn an unknown type into a scalar or wildcard.

## Project Metadata From Rust

The scalar adapter derives its description from the same immutable registration
used by compilation and dispatch:

```rust
use leselang_vm::embedding::{HostBuilder, ScalarType, ScalarValue, parameter};

let calls = std::cell::Cell::new(0);
let mut builder = HostBuilder::new();
builder.register(
    "document.replace",
    vec![parameter("body", ScalarType::String)],
    ScalarType::None,
    "document.write",
    |_, _| {
        calls.set(calls.get() + 1);
        Ok(ScalarValue::None)
    },
)?;
let host = builder.build();
let description = host.catalog_description("myapp.document");
let json = description.to_json()?;
assert!(json.ends_with('\n'));
assert_eq!(calls.get(), 0);
```

`Host::describe()` remains the original borrowed Rust view. The new
`catalog_description(profile)` returns owned
`native::core::discovery::CatalogDescription` data. The caller chooses the
profile label. The owned projection can outlive its host and be edited, but
editing it never alters registration, callback behavior, grants or a compiled
program. It is not an alternative input to `Host::start` or dispatch.

Native adapters construct the same owned DTOs from their actual operation
schemas, `source_type_name` aliases and scalar field declarations. AppKit, AX
and Tauri use shared schema/type tables for compilation and discovery. An adapter must
project the real required capability policy rather than invent a second
registration list. GUI labels such as `gui.read` and `gui.write` describe the
host's existing native policy; they do not introduce a GUI CLI `--allow` flag.

`CatalogDescription::to_json()` validates the complete projection before
returning a single bounded string. Duplicate declarations, invalid names,
undeclared native references and duplicate type alternatives are rejected.
Neither the original `OperationCatalog` nor any `ObjectRef`, session, request
or receipt gains serialization. There is no deserializer that restores
execution authority from a description.

## Bounds And Failure Handling

The serializer has fixed inclusive presentation limits:

| Item | Limit |
| --- | --- |
| Encoded UTF-8 record, including escapes and final LF | 256 KiB |
| Operations | 64 |
| Parameters per operation | 64 |
| Native type declarations | 32 |
| Fields per native type | 64 |
| Type alternatives per parameter | 38: the six scalars plus at most 32 declared native aliases |
| Profile label | 1–128 ASCII letters, digits, `_`, `-` or `.` |
| Operation name | 1–128 total bytes, with dot-separated ASCII identifier segments |
| Parameter label | One ASCII identifier, 1–64 bytes |
| Native helper alias | One non-reserved ASCII identifier, 1–64 bytes |
| Literal field name or capability label | 1–128 UTF-8 bytes |

Operation and parameter identifiers start with a letter or underscore and
continue with letters, digits or underscores. The tokens `fn`, `true`, `false`
and `none` cannot be identifier segments. `body` remains a valid operation or
parameter label; native helper aliases additionally exclude `body` and all six
scalar names. Names compare exactly, without normalization. Field names and
capability labels retain Unicode text; quotes, backslashes, control and
directional characters are escaped in JSON. Decoded text still needs the
escaping appropriate to its display destination.

Count limits are checked before traversing each corresponding collection.
Hosts still bound prior allocation and projection work. A custom native or
scalar registry can be valid yet exceed the description limits; projection
does not silently truncate or weaken that registry. Encoding failure returns
`DescriptionError` with no partial string. The encoded byte cap includes escape
growth and the final LF, independently of the source and runtime scalar bounds.

For `leselang describe`, registration or encoding failure exits 1 with
`LSD0001`; writing or flushing standard output failure exits 1 with `LSD0002`.
JSON diagnostics use phase and kind `discovery`, with no path, source span or
remaining Fuel. A write failure can occur after partial external output, so it
is reported without retry. Invalid command-line options exit 2 under the usual
usage diagnostic contract. The native host CLIs retain their own error
presentation; all three share the catalog record schema.

The description is useful for choosing a source profile and presenting a
permission request. Compilation, scope selection and live authorization still
happen through their existing APIs. It carries no live grants, native instance
keys, object generations, observations or replies. This metadata format does
not define remote execution, transport authentication, version negotiation,
replay or durable recovery. See the [local host protocol](host-protocol.md) and
[embedding guide](embedding.md) for those boundaries.
