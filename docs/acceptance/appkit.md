# AppKit Acceptance Records

These dated records retain their original application, platform, source and test
scope. Historical counts describe the named milestone, not the current suite.
Follow the linked source guides for current setup and contracts; see the
[acceptance index](README.md) for other components. Paths under `target/` identify
local ignored evidence and are not distributed artifacts. References within a
record to “this guide” or “above” retain the context of its linked source guide.

## Native Form And Typed Helpers

Source guide: [appkit-host.md](../appkit-host.md).

The local form run was also checked through the real window's accessibility
tree on 2026-11-13: input and saved values were both `LeseLang ✓`, Save showed
`ready` with one callback, and Other remained `draft` with zero callbacks. That
run reported nine Effects and 508 Fuel remaining. This desktop inspection
complements the native state assertions and failure tests.

On 2026-11-13, the updated main-thread `--self-test` passed all 16 groups,
including the typed-input/native-helper group. That group executed the same
text-task source twice, first with `typed native helper ✓` and then with
`second task input`. Each run returned `true` after four Effects, and fresh
observations plus direct native assertions confirmed the actual text field's
value. The group's invalid-input and forged-native-type rejection checks also
passed; the existing 32 stale-state, authority and dependency cases remained
part of the successful run.
