# Development Checks

`check.sh` is the shared local and CI entry point. It runs from the repository
root even when invoked from another directory. Select a group rather than
copying a second list of Cargo commands:

```sh
sh scripts/check.sh --list consumer
sh scripts/check.sh consumer
sh scripts/check.sh javascript
```

Use `--help` for every group or `--list all` to inspect the checks applicable to
the current platform. Listing performs no builds and creates no directories.
`all` runs the groups sequentially and stops at the first failure. Individual
commands and their full output remain visible; failures retain their exit code.

Install the pinned Rust and npm dependencies as described in
[the testing guide](../docs/testing.md) first. The runner never fetches or
installs dependencies. Cargo checks use the repository's pinned toolchain,
`--locked --offline --profile lean` and an explicit shared `target/` directory.
This prevents environment target overrides from creating a second cache or
causing offline plan checks to run a different executable.
The optional registered GUI benchmark builds its native artifacts with `--release`.

| Group | Coverage / prerequisite |
| --- | --- |
| `tensor` | Standalone Rust development tool, checked catalog and evidence identities, hierarchy and task selection |
| `language` | Main workspace formatting, all-feature tests and strict Clippy |
| `consumer` | Independent Rust suite, catalog-agent example tests and Clippy |
| `scalar-consumers` | Python, C11 and C++17 meter hosts against the real worker, covering success and two failure vectors. Requires Python 3.8+, C and C++ compilers, pkg-config and cJSON development files. [Setup and scope](../docs/operations-host-protocol.md#python-c-and-c-consumers) |
| `dotnet-consumer` | Worker challenge and reply checks, native .NET 10 Consumer, shared two-Effect fixture and failure paths. Windows uses Git Bash, MSVC and native `.exe` files. Restore uses the installed .NET reference packs and a local source. |
| `registered` | Standalone Rust host/worker, dependency boundary, strict Clippy and a separate Rust Consumer |
| `registered-gui` | Compare Rust, Node and C# on the [shared registered GUI fixture](../fixtures/registered-gui/README.md). Requires the SDK and Node Consumer npm dependencies, pinned Cargo dependencies and .NET 10. Writes each Consumer's observations under `target/verification/gui-consumers.*`. |
| `benchmark-registered-gui` | Opt-in Release timing of the same three Consumers, including successful, failed and cancelled tasks. Accepts `--warmup N --samples N`. Add `--worker-phases` for the Node and C# worker breakdown. Run separately from `all`. [Method and baselines](../benchmarks/README.md) · [Worker phase timings](../benchmarks/worker-phases.md) |
| `javascript` | SDK, Node transport failure fixtures, packed JS/TS Consumer, browser isolation and Tauri frontend; install SDK and host npm dependencies |
| `node-consumer` | Both portable Rust carriers, checked application declarations, real Node application, packed external Consumer with real VM, and an offline Tauri agent plan; requires SDK build/development tools and Node Consumer npm dependencies |
| `appkit` | Formatting, non-interactive package tests, Clippy and host executable |
| `macos-ax` | Formatting, fake-backend tests, Clippy and host executable; native bridge boundaries also run on macOS |
| `tauri-portable` | Formatting, tests and Clippy without desktop features |
| `tauri-desktop` | macOS transport build, tests and Clippy; requires generated frontend assets and loopback sockets |
| `smoke` | CLI/embedding/cancellation examples and AppKit/Calculator offline plans; run `appkit` and `macos-ax` first |

The `node-consumer` and `smoke` groups write plans into new subdirectories under
`target/verification/plans.*`, printing the parent path. Existing evidence is
never overwritten. Offline plans do not execute GUI actions. Logs from local
checks can be redirected under `target/verification/` as well.

Interactive GUI acceptance, Accessibility permission changes and external model
calls are not part of these groups. The desktop integration test that explicitly
opens real windows stays ignored unless invoked separately. Read
[the testing guide](../docs/testing.md) and [acceptance archive](../docs/acceptance/README.md)
for the distinction between automated checks and observed desktop behavior.
