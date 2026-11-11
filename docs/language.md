# LeseLang Scalar Language And CLI

The `leselang` executable checks and runs the current scalar language profile.
It uses the same compiler and session engine as Rust embedding. Programs are
UTF-8 source files containing `fn` declarations and a parameterless `main`.
The `.lese` extension is a convention; the CLI accepts any file path.

## Run A Program

From this checkout, with the pinned Rust toolchain and dependencies fetched:

```sh
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- check examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/two_effects.lese --allow stdio
```

The arithmetic program prints `42`. The Effect program prints `first` and
`second` on separate lines. To use the executable directly, build it with
`cargo build -p leselang-vm --bin leselang --locked --offline --profile lean`;
it is then available at `target/lean/leselang`.

```text
leselang check FILE [--allow stdio]
leselang run FILE [--allow stdio] [--fuel N]
leselang --help
leselang --version
```

Use `-` as the input path to read source from standard input. Use `--` before
a path beginning with a hyphen. `-h` and `-V` are aliases for help and version.
The source limit is 16 KiB. `run` starts with 10,000 units of Fuel by default;
`--fuel N` supplies an unsigned integer budget, including zero.

`check` parses, resolves operations and checks types and limits without invoking
host callbacks. It is quiet on success. `run` executes the program and prints its
final scalar value as a JSON literal, except that a final `none` produces no
extra line. String results are escaped and quoted; lists use JSON arrays. An
absent `optional_string` prints `null`, while a present one prints its quoted
string. `std.print` writes its text directly, without result-value quoting. Diagnostics go to
standard error. Exit status is `0` for success, `2` for command-line usage
errors, and `1` for input, compilation or execution failures.

## Expressions, Names And Helpers

Every function body is one expression. Calls use named arguments, including
arithmetic; there are no infix `+` or `*` operators. Argument labels must match
the declaration and may be written in a different order. Comments start with
`//`; line breaks and indentation are whitespace, and a trailing comma is
allowed in a call.

```text
fn double(n: integer) = mul(left: n, right: 2)
fn main() = double(n: 21)
```

Helper parameters require one of the scalar type names below. Return types are
inferred. Helpers have their own local scope, may call other helpers and may
contain host operations. Recursive or mutually recursive helpers are rejected.
The compiler checks unused helpers and unchosen branches too: unreachable code
does not hide unknown operations or type errors.

`bind` names a result for its `body`. The first argument's label is the new
local name; `body` is reserved by this form. A binding can hold a pure result or
a host Effect's reply. Names are immutable and cannot shadow an active local.

```text
fn main() = bind(
    answer: add(left: 19, right: 23),
    body: to_string(value: answer),
)
```

## Scalar Values

| Type | Example | Meaning |
| --- | --- | --- |
| `integer` | `42` | Unsigned 64-bit integer, from 0 through 18446744073709551615. |
| `boolean` | `true`, `false` | Boolean value. |
| `string` | `"hello"` | UTF-8 text, at most 4,096 bytes. |
| `none` | `none` | A unit value. |
| `optional_string` | `optional_string(value: "hello")` or `optional_string(value: none)` | Present or absent text; absence is distinct from an empty string. |
| `string_list` | `strings(first: "a", second: "b")` | Ordered strings, at most 64 items and 4,096 total UTF-8 bytes. |

There are no signed integer or floating-point values. Arithmetic is checked:
overflow, subtraction below zero, and division or remainder by zero fail rather
than wrapping. String literals support `\"`, `\\`, `\n`, `\r` and `\t`
escapes; Unicode characters may be written directly.

`strings()` makes an empty list. Its argument labels must be unique, but they
are not stored as list keys; values retain source order. Lists contain strings
only. Optional values and lists are constructed through these functions rather
than special bracket syntax.

## Pure Operations

All binary operations below take `left:` and `right:`; unary operations take
`value:`. Values are not coerced between types.

| Operations | Accepted inputs and result |
| --- | --- |
| `add`, `sub`, `mul`, `div`, `rem` | Two integers → integer. |
| `lt`, `le`, `gt`, `ge` | Two integers → boolean. |
| `eq`, `ne` | Two values of the same scalar type → boolean. |
| `and`, `or`; `not` | Booleans → boolean. `and` and `or` short-circuit their right operand. |
| `concat` | Two strings → string. |
| `contains`, `starts_with`, `ends_with` | Two strings → boolean. |
| `len` | String or string list → integer. String length counts Unicode scalar values, not bytes or grapheme clusters. |
| `char_at` | String and zero-based integer index → optional string. |
| `split` | String and separator string → string list. |
| `join` | String list and separator string → string. |
| `append` | String list and string → string list. |
| `item_at` | String list and zero-based integer index → optional string. |
| `optional_string`; `has_value` | Wrap string/none as optional string; test an optional string for presence. |
| `value_or` | Optional string and fallback string → string. The fallback is evaluated only when absent. |
| `to_string` | Integer, boolean or string → string. |
| `parse_integer`; `parse_boolean` | String → integer or boolean. Integer parsing accepts ASCII decimal digits only; boolean parsing accepts exactly `"true"` or `"false"`. |

Out-of-range `char_at` and `item_at` return an absent optional string. Character
indices, like string `len`, address Unicode scalar values.

## Branching And Recovery

`choose` takes a boolean `when` and two same-type branches. It evaluates only the
selected branch. A branch may contain a host Effect, but the condition must be
pure.

```text
fn main() = choose(when: true, then: 42, otherwise: 0)
```

`recover(value: ..., fallback: ...)` handles arithmetic failures and invalid
integer/boolean text from pure expressions. Both expressions must have the same
scalar type. It evaluates the fallback only when the value produces one of
those recoverable failures.

```text
fn main() = recover(value: div(left: 1, right: 0), fallback: 42)
```

It does not catch type errors, Fuel exhaustion, size/iteration limits, revoked
permissions or host failures. Host operations cannot be placed inside `recover`.
See [choice_recovery.lese](../examples/choice_recovery.lese) for a complete
program combining the forms.

## Bounded Loop And Fold

`loop` maintains one scalar state. The first argument label names it, `while`
is a pure boolean condition, and `next` produces a replacement of the initial
state's type. The result is the state when the condition becomes false.

```text
fn main() = loop(
    n: 1,
    while: lt(left: n, right: 16),
    next: mul(left: n, right: 2),
    limit: 4,
)
```

`limit` is a literal integer from 0 through 1,024, bounding successful updates.
If another update is needed after that budget, evaluation fails. Fuel is a
separate budget charged during the loop.

`fold` traverses a string list in order, carrying one scalar accumulator. Its
first non-reserved argument label names the accumulator. `item` is a literal
string naming the local variable for the current list entry. `next` must retain
the accumulator's type. An empty list returns the initial value.

```text
fn main() = fold(
    total: 0,
    items: strings(first: "Lese", second: "Lang"),
    item: "word",
    next: add(left: total, right: len(value: word)),
    limit: 2,
)
```

This returns `8`. A fold's literal `limit` ranges from 0 through 64 and must
cover the list's item count. Initializers, conditions, collections and update
expressions in these scalar forms must be pure; pure helpers are allowed.
Perform a host Effect with `bind` outside the loop or fold, then use its scalar
reply. See [bounded_loop.lese](../examples/bounded_loop.lese) and
[fold_strings.lese](../examples/fold_strings.lese).

## Host Effects And CLI Permissions

The CLI registers one reference host operation:

```text
std.print(message: string) → none
```

Grant it with `--allow stdio` on either `check` or `run`. Without that grant,
programs referring to it fail compilation, including references in cold
branches or unused helpers. The grant permits standard output only; the CLI
provides no filesystem, network or process operations to the language.

```text
fn main() = bind(
    first: std.print(message: "first"),
    body: std.print(message: "second"),
)
```

The host handles each operation through explicit `poll → authorize → dispatch
→ accept` before the next language slice runs. If a later expression or write
fails, earlier output remains visible. `std.print` charges the UTF-8 message
length plus one unit before writing, appends a newline and flushes. An I/O error
is classified as an uncertain host outcome because some output may already
have been written. Fuel bounds accounted work and does not preempt arbitrary
native code. No asynchronous scheduler is installed.

This CLI profile has no modules/imports, recursive functions, mutable global
variables, arbitrary objects or `seq`/`all`/`repeat` orchestration forms. The
broader native HIR protocols remain available to custom Rust hosts. To register
your own operations or adapt resources and event loops, use the independent
[Rust embedding guide](embedding.md).
