# Repository Handoff

## Origin

This repository was initialized from Gewyvern's reviewed native source exporter
on 2026-10-28. The source checkout HEAD was
`784e4aac7ca622497de40333ab1d4ade5787f84d` (commit subject `2.5.5`). Its Cargo
workspace version was 2.5.0. Independent Leselang now starts at **0.1.0** instead
of inheriting that product version; Gewyvern's own version is unchanged. Pending
product test ownership changes were present; this is a recorded working-tree snapshot, not
an assertion that every source byte came from that commit alone.

[source-checkpoint.json](provenance/source-checkpoint.json) is the original
export inventory. Paths and hashes describe the export **before repository
onboarding changes**, not the forever-current tree. Its `verified = false`
records that export invocation; subsequent repository test runs do not rewrite
historical evidence. [The baseline notes](provenance/extraction-baseline.md)
retain the original acceptance scope, not current repository setup instructions.
The bootstrap verification receipt likewise describes the pre-0.1.0 snapshot;
its version and hashes are historical evidence, not the current package metadata.

Production implementation and test bodies are imported unchanged. Repository
onboarding adds documentation, toolchain/configuration, CI and a dependency
boundary test; facade documentation is made native-only. The two facade `lib.rs`
files drop unreachable product re-exports while preserving their native exports
and compile-fail documentation. No evaluator, syntax or protocol is reimplemented.
The original MIT LICENSE and copyright notice are retained byte-for-byte.

## Ownership During Handoff

Native foundation development moves to this repository. Gewyvern temporarily
retains its existing compatibility baseline so an ordinary checkout still builds
before a real Leselang revision is available. Do not regenerate this repository
from the old exporter or develop a second native implementation in the product.
Any temporary backport must be explicit.

The product retains `leserpent-hir`, `leserpent-vm`, command/host-contract/UI/
observation adapters, journals and old feature-enabled compatibility entry points.
Names beginning with `leselang-` in that repository are not automatically generic
language code and must not be moved wholesale.

## Complete The Product Cutover

1. Review and make the first Leselang commit; choose/create its remote explicitly.
2. Pin the three shared cores (`leselang-syntax`, `leselang-runtime-core`,
   `leselang-hir-core`) to the same immutable source in every product consumer.
3. Keep the old HIR/VM product facades until their import paths are deliberately
   migrated. They must re-export the same shared types, not compile a duplicate
   source identity or restore product features in this repository.
4. Run the product HIR/VM, command, daemon/CLI and cross-facade identity/recovery
   suites against the pinned source, including no-SQLite and native profiles.
5. Remove obsolete in-tree native implementations/export assumptions and update
   product CI/status ownership in the same reviewed dependency-switch change.

A permanent sibling-directory dependency is not suitable for the product's
ordinary clone or CI. No remote, commit, tag or registry release is fabricated
as part of creating this local repository. The user controls Git publication.

## Separate Release Gates

Repository independence does not imply complete declared-language parity,
production host-neutral durable graph recovery, a stable FFI ABI, automatic GUI
adaptation or a crates.io release. `complete_vm` and `full_extraction_gate` remain
false in the historical checkpoint. Continue that acceptance work here without
making it a prerequisite for the existence of the independent repository.
