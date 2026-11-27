# Host Lese operations in C#

A .NET application owns its commands, grants and state. It can expose those
commands through the same `lese.operations` catalog as a Node application.
One standalone Rust worker compiles and runs each submitted Lese program. The
handlers execute in the application process so they can call the application's
own services.

## Run the independent Consumer

Use the .NET 10 SDK and the repository's [Rust prerequisites](testing.md).
On Windows, use Git Bash and the MSVC Rust toolchain with the Visual Studio C++
build tools and Windows SDK. From the checkout root, run the same command on
Windows, macOS or Linux.

```sh
sh scripts/check.sh dotnet-consumer
```

The check tests the worker's challenge and reply handling, then builds and runs
the independent C# application. On Windows it starts the native
`leselang-agent-worker.exe` and `LeseLang.Consumer.exe` with absolute Windows
paths. Each successful check prints its result, including the OS, balance,
Effect count, remaining Fuel and failure-path checks. The C# library
uses only the .NET base class library. Restore uses a local package source,
and managed build outputs go under `target/dotnet/`. The C# project is available
as a [project reference](../packages/dotnet/LeseLang.Operations.csproj).

## Declare and call an operation

The following method is an example for an application that keeps a list of
items. Its caller supplies an absolute worker path and a live policy function.

```csharp
using LeseLang.Operations;

static async Task<bool> AppendAsync(string worker, List<string> items, string text,
    Func<bool> canWrite, CancellationToken cancellation)
{
    var registry = new OperationRegistry([
        new("items.append", new Dictionary<string, string> { ["text"] = "string" },
            "integer", "items.write", (args, context) =>
            {
                context.Commit(() => items.Add((string)args["text"]!));
                return ValueTask.FromResult<object?>((ulong)items.Count);
            })
    ]);
    var host = new OperationHost(worker, registry, () => canWrite() ? ["items.write"] : []);
    var catalog = host.Describe(); // Give this JSON value to a planner.
    var before = items.Count;
    var report = await host.ExecuteAsync("fn main(input: string) = items.append(text: input)",
        new Dictionary<string, string> { ["input"] = text }, cancellation);
    return report.Effects == 1 && report.ResultType == "integer"
        && report.Result.GetString() == items.Count.ToString(System.Globalization.CultureInfo.InvariantCulture)
        && items.Count == before + 1 && items[^1] == text;
}
```

`OperationHost.Describe()` returns `schema_version`, `profile` and `operations`.
Each operation has a name, named parameters, a result type and a required
capability. The compiler checks the selected entry and its grants before the
first callback. A handler receives typed values in a read-only dictionary.

| Declared type | Value in the handler | Value in the JSON report |
| --- | --- | --- |
| `integer` | `ulong` | Canonical decimal string |
| `boolean` | `bool` | Boolean |
| `string` | `string` | String |
| `none` | `null` | Null |
| `optional_string` | `string` or `null` | String or null |
| `string_list` | `IReadOnlyList<string>` | Array of strings |

The registry uses the [shared scalar and signature bounds](application-operations.md).
Source is limited to 16 KiB. Entry arguments are zero to eight named strings,
with a 4096 UTF-8 byte limit per string and 16 KiB in total. Return an explicit
`ulong` for an integer handler, including values beyond JS's safe integer range.

## Execution and application state

A host admits one execution at a time. It snapshots grants for compilation and
checks current policy on dispatch, commit and reply. Pass the caller's
`CancellationToken` to `ExecuteAsync` and use `context.CancellationToken` for
awaited handler work. After an await, enclose the synchronous application change
in `context.Commit`. Applications that require a UI thread must marshal the
commit onto their own dispatcher. Host handlers own scheduling and the cost of
their native work.

The worker starts with 10,000 Fuel and permits up to 256 exchanges, with a
five-second budget per Effect. The parent bounds the worker lifetime to 15
seconds. These budgets apply to the worker and cooperative code. A blocking
application callback must implement its own work limit.

`ExecuteAsync` returns VM evidence after a complete report and clean worker exit.
The application compares that evidence with a fresh read of its state. Use the
application's own revision or receipt when other callers may change the same
state. On cancellation, a malformed handler result or a lost worker, execution
can end with an exception after a mutation has committed. Read the state before
deciding to submit another operation. The host never automatically replays a write.

## Compare the three hosts

The [shared fixture](../fixtures/application-operations/two-effects.json) specifies
the signatures, source, grants, arguments and expected result for one meter.
The Rust Consumer embeds the VM directly. Node and C# use the same worker with
the neutral profile. Each host starts at `9007199254740992`, adds one, and reads
back `9007199254740993`. The fixture also checks two Effects and 9,994 Fuel
remaining. See the [Rust test](../consumer/tests/embedding.rs),
[Node test](../consumer/javascript/agent/operations-conformance.test.mjs) and
[C# Consumer](../consumer/dotnet/Program.cs).

All three Consumers also run the meter with only the read grant, then in a
fresh application with both grants and a write handler that commits before
returning a string. Each checks the failed outcome, both callback counts, the
number of committed writes and the exact balance. These assertions describe
the application state even when execution ends without a successful report.
[Shared failure vectors](../fixtures/application-operations/failure-vectors.json)

The worker generates a fresh 32-byte challenge for each request. Windows uses
the system-preferred random generator through `BCryptGenRandom`. macOS and
Linux read from `/dev/urandom`. All three encode the challenge as 64 lowercase
hexadecimal characters. An OS randomness failure stops that exchange before
the request is sent. The .NET host flushes each startup and reply frame before
waiting for the next worker message.

The [native Windows acceptance record](acceptance/agents.md#2026-11-27--native-windows-worker-and-c-consumer)
links the exact tested commit and the three platform jobs.
