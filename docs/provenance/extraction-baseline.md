# Leselang Native Source Checkpoint

This self-contained MIT source workspace contains the production syntax, runtime
foundation, host-defined HIR/in-memory sessions and native VM facade, plus their
unchanged native tests. Production Rust files are copied byte-for-byte, not independently reimplemented.
Crate manifests own explicit metadata and pin the original locked registry closure.
No Gewyvern/Leserpent product crate, account service, generated asset or SQLite is
required. The reference profile is not an enableable Cargo feature here; all-features
builds stay native. Product-only tests and reference implementation files stay in
the original repository. Mixed tests retain their original unreachable cfg guards.

## Try the native host

Run `cargo run --example native_counter --locked --offline --profile lean` here,
or add `--manifest-path consumer/Cargo.toml` for the independent consumer.
The normal source/helper/capture program returns 42 after two calls; append
`-- --cancel` to stop after the first effect (19). `--deny` and `--revoke`
intentionally fail, before execution or after the first effect respectively.
Failure output includes partial native state and never implies rollback.
Run `cargo test --example native_counter --locked --offline --profile lean`
in either workspace for the twelve bounded safety/lifecycle regressions. The same
example source uses only std and the public native facade; no test harness,
product DTO, second evaluator or durable storage is involved. This is a small
atomic scalar host, not the complete language/recovery gate.

Append -- --queued to interleave two local VMs sharing immutable IR, with
independent owners, state and original fuel. The host owns a two-event queue
and a 64-turn ceiling; dispatch and reply acceptance happen on separate turns.
--queued-cancel-pending cancels before mutation (zero effects), whereas
--queued-cancel-reply discards a late receipt after one committed effect (19).
--queued-revoke refuses live reentry and intentionally exits with failure.
The sibling completes at 42 in every mode. Idle polls, foreign/wrong-typed
receipts and isolated exhaustion are tested without a second evaluator.
This is a single-threaded headless host loop, not async language semantics,
a real GUI integration, thread-safety guarantee or durable recovery.

The exported leselang-vm package exposes native::{syntax, hir, core, Vm, LocalVm}; Vm is
the exact shared StackedControlSession. Default and all-features builds here stay
native. LocalVm adds opt-in local lifetime ownership. Its with_host_work boundary
protects adapter work while Awaiting: normal native refusals stay verbatim, escaping
unwind seals HostUncertain and revokes before retained pending cleanup. Hosts still
check live authority and own work limits, cancellation and rollback.
LocalVm reply acceptance uses shared PendingReply::try_accept_in_scope to revoke
before core-owned identity/frame/reply cleanup on validation unwind. Ordinary
rejection with an open scope retains original input and liveness; callbacks remain trusted.
Synchronous callback revocation stops later reply checks and seals Cancelled before
superseded-error/frame cleanup. A normal cancelled rejection never rearms the shared
session; interrupted cleanup keeps the enclosing session HostUncertain and closed.
Legacy/prepared Seq and All expose explicit try_accept_in_scope entries through
original group reply cores. Revocation discards unpublished suffix/results; normal
refusal preserves input and state. Parent-session and external dispatch cancellation
remain host-owned; the collector does not gain LocalVm's private scope.
Explicit poll_in_scope gates publication and ordered All result handoff, including
ReadyResults after all receipts arrived. Revocation stops preparation/identity hooks
and seals before retained cleanup. Failure keeps native errors and spent fuel;
callback unwind stays HostUncertain. Start/correlation and dispatch remain host-owned.
LocalVm polling also protects retained re-entry owners, restored-prefix rejection
and successor-correlation failure. Language-recovered scalar faults stay live with
original fuel; consumed native inputs and deeper temporaries remain host-owned.
Use with_host_outcome and HostWorkOutcome to classify normal returns as Pending,
Cancelled, Failed or Uncertain without panic or error-code inference. Both entries share the
original pending state and preserve native outputs; terminal classification
revokes before retained cleanup. Cancelled(output) hands host collectors back after
revocation for explicit cleanup; partial native effects are not rolled back. Upstream product-profile documentation describes the omitted reference
specialization, not an enableable feature of this checkpoint. The independent
consumer runs its original two-host source/capture/reply/cancel/fuel tests unchanged.

Use Rust 1.98.0 with Clippy. From this directory run:

```text
cargo test --workspace --all-features --locked --offline --profile lean
cargo clippy --workspace --all-features --all-targets --locked --offline --profile lean -- -D warnings
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path consumer/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
```

Offline builds require the locked registry dependencies to be cached first. The
standalone CI fetches that closure, then uses locked/offline verification. The
consumer is a separate workspace and exercises owned cleanup, accepted reply
sessions, nested returns and a headless GUI adapter without monorepo source paths.
The continuous native_host_pipeline matrix also runs unchanged: distinct scalar and
structured replies traverse bounded control, helpers, two effects, capture, every
fuel cutoff, live-policy refusal, stale receipts and isolated cancellation. The mixed matrix also
traverses atomic calls, seq/all/repeat, captured successors and a second group
through original prepared collectors and one meter. Twelve host/program cases
cover reverse replies, group provenance/order/count, cancelled prefixes and shared-IR
isolation. Sixteen conditional host/program paths additionally select group-returning
helpers from an earlier receipt, alias the group and either return early or capture
one more effect. Direct Field(Member) compilation keeps original bounds and cold
export checks; unselected arithmetic is not evaluated. Every fuel cutoff and
early-return authority revocation run through the same native implementation.
Seventy-two selected-data paths join atomic/group helpers and pure fallbacks
for all six data types before another captured effect. Sixty empty/absent/maximum
optional/list payload paths retain values through re-entry; computed overflow
prevents the caller tail. Cold types, unused argument laziness, revoked accepted
receipts, original reply ownership and exact fuel use the existing implementation.
All 30 scalar operators and 44 legal type signatures now cross two host reply types,
Seq/All and atomic/group/pure return routes: 528 successful paths retain each
computed value across another captured effect. Exact completion thresholds, 1440
cold shape/type refusals, 108 dynamic faults and 60 lazy/recovery paths use the
same compiler and evaluator; fixed expected values do not call the tested operators.
Unused unknown names still fail cold. This is a bounded scalar acceptance matrix.
The typed-control matrix covers all six scalar helper/state types through eight
forms and 576 two-host paths, including zero/empty iterations, nested fold/loop
and permitted arithmetic recovery. Another 288 limit failures cannot recover into
a successor; 648 cold type/purity/scope refusals precede host work. Exact completion
thresholds and hygienic helper locals share the original compiler and evaluator.
The singleton none state exits successful conditional loops immediately and is
separately checked under a nonterminating bounded loop. This is typed-control
acceptance, not arbitrary program or durable recovery coverage.
The declaration matrix sends all six scalar types through an eight-parameter
helper in 432 named-order/forward-reference paths with exact fuel thresholds.
576 unused-argument probes distinguish declaration-order faults from written
argument order and unselected calls. The fixture admits the existing 32-function
ceiling: 72 complete programs leave unused faulting helpers idle, while 936 cold
ceiling/signature/scope/recursion refusals precede host work. Production limits
and runtime implementations are unchanged. This is declaration acceptance only.
Prepared conditional helpers also traverse seq/all/repeat/nested-seq and six
return types in 192 paths, preserving named expansion and exact fuel thresholds.
320 dynamic failures at eight source sites close before any invocation or root/
member identity; 40 cold refusals exclude non-atomic or caller-capturing members.
Repeated source leaves fail at their first evaluated copy, not an arbitrary later
iteration. This bounded seven-member proof reuses the original preparation and
collectors; it does not provide full branch-ceiling coverage or effect rollback.
The branch-ceiling matrix now reaches 64 members across seq/all/repeat/nested-seq,
including 63 members plus a captured successor, in 32 two-host paths. 64 cold
refusals and 40 preparation faults cover oversized/hidden groups, stricter host
limits and first/middle/final source members. Exact completion and first-request
fuel cutoffs stay bounded. Correlation may consume identities but cannot dispatch
a failed request; terminal scope closure is not identity or effect rollback.
The fixture admits 64 source arguments and 128 cold calls (both alternatives),
not a new product effect-slot policy. Production limits/runtime are unchanged.
Whole-program budgets now compose two hosts and seq/all through scalar/group
helpers and a captured successor: 52 compiler and 24 VM threshold cases reject
required-minus-one and execute at the exact budget and one above it. 152 invalid
configuration refusals follow 76 legal safety-ceiling controls. Twelve unused
body paths and 36 refusals cover nonrefunding work, the fixture 1024-node bound
and parser depth 16, distinct from IR depth 64. Host preparation retains its own
operand scope policy; VM limits are not native callback sandboxes. This verifies
these composed programs, not every maximum-size shape or complete extraction.
The host-owned disk fixture drops the old VM, IR, catalog and owner before
reopening eight two-host pending-call paths through LocalVm and PreparedCall.
Original frames/operands/fuel and recorded receipts preserve exact completion
thresholds, reject stale owners and avoid re-dispatch of recorded effects.
It covers 24 budget paths, 96 candidate correspondence refusals and 16 live
authority refusals. Serde is a VM test-only dependency. This is same-process
disk reopen, not process restart, power-loss, storage authentication, a production
journal, effect-to-receipt crash atomicity or complete group-graph recovery.
The disk All extension restores one three-member group and nested parent frames
under a shared scope, then projects results and dispatches an atomic successor.
Two hosts, 15 dispatched-prefix/accepted-subset frontiers and six receipt orders
give 180 successful paths; 90 exact-budget cases, 120 owner refusal/cancellation
cases and 240 candidate correspondence refusals preserve original execution.
Recorded settled effects do not redispatch, and terminal replay keeps the ledger
unchanged. This same-process fixture has scalar captured locals at its cut; it
does not restore predecessor graphs or Seq, provide a general journal, authenticate
storage or test process/power-loss crash consistency. The original cores are reused.
The shared disk fixture also restores Seq's accepted prefix, at most one awaiting
request and its unpublished suffix. Twenty two-host reopen paths cover ten cuts
per host; awaiting work is never republished, acceptance releases no successor,
and each queued publication rechecks live policy. Sixty completion-budget paths,
80 owner refusals/cancellations and 24 queued-publication refusals preserve the
original cores and host ledger. Six bad host-written frontiers fail structurally
before complete-batch admission, separately from 160 altered candidate refusals.
The initial Seq proof used private fixture format 2. Format 3 then added
one completed predecessor group, shared aliases and a selected member to
the original pending Seq/All and parent-frame recovery path. Two hosts cover
200 successful reopens, 150 exact-completion budget paths, 300 owner refusal/
cancellation paths and 48 corrupt host-written graph/alias records. Original
collectors plus restore_group_reply rebuild one alias owner without replaying
predecessor effects or charging completed assembly again. This covers seq-to-seq
and all-to-all, not arbitrary mixed graphs. Durable cancellation, authenticated
storage and external-effect crash consistency remain host contracts or open
acceptance gates, not consequences of local scope closure or this test codec.
The private format 4 acceptance checks two predecessors and a pending group
in all eight Seq/All kind combinations. Original IR/root/kind and frame/name/
member origins remain host-owned ledger data, not consequences of schema equality.
Each completed group has one distinct alias owner; original pending collectors
refuse earlier-group identities without consuming the caller reply buffer.
Two hosts cover 800 successful reopens, 600 completion-budget paths, 1600 owner
refusals/cancellations and 192 corrupt graph/alias records, using the same original
VM, collectors and assembly restoration. This bounded three-group topology
is not arbitrary graph recovery, storage authentication, durable cancellation
or a process/power-loss guarantee. Historical proofs retain their own hashes.
Private fixture format 5 adds explicit Completed/Failed/Cancelled outcomes and
a host-owned cancellation intent. The restored scope closes before intent and
terminal publication; only a successful terminal write acknowledges cancellation.
A committed intent fences ordinary reopen and stale adapter dispatch until an
authorized cancellation retry finishes. Existing success/failure terminals win
over late cancellation. Six tests cover 300 acknowledged paths, 300 interrupted
terminal paths, 600 pre-intent refusals, 600 prior terminal winners, 600 stale
adapter refusals, 24 inconsistent terminal records and four failed-restore cases.
Bounded temporary files are file-synced before rename, with injected failures
before publication. This serialized same-process fixture is not a concurrent
transaction, authenticated store, external cancellation delivery or crash proof.
Before intent commits there is no persisted fence or acknowledgement; hosts must
resolve that uncertainty. Production runtime and journal protocols are unchanged.
Cross-process acceptance now runs this same Rust test executable directly,
without shell commands or a second evaluator. Two hosts, two mixed predecessor
pairs and seven pending cuts cover 84 exact-budget three-process chains,
28 confirmed-cancellation chains, 28 intent-exit/retry chains and eight refusal
chains: 464 child processes in that initial slice. Each reader has a fresh process
and host; reports must match actual child/parent PIDs and phases. The intent
writer exits with code 73 before terminal publication and stack cleanup, leaving
ordinary restart fenced until explicit cancellation retry. Child deadlines,
output checks and kill/reap cleanup bound the driver; only the parent owns the
temporary directory. Private checkpoint format 5 is unchanged. This tests real
process restart and a controlled exit boundary, not SIGKILL, mid-write failure,
power loss, concurrent writers, external-effect atomicity or complete extraction.
Full language parity, durable recovery and reference compatibility remain open.
Counter and GUI prepared-repeat programs run unchanged in this separate consumer,
including explicit-Seq fuel parity, original reply ownership and revocation fences.
Saved-frame and parsed durable-control consumer models recover original budgets,
checkpoint watermarks and settled success/refusal receipts through the native
compiler and walker. Host-admitted saved helper return stacks now pass bounded frame/name/whole-forest
checks before reusing PendingReply and the original stacked re-entry walker. Two
native byte-codec hosts restore three saved frames and continue three effects
without recompilation, budget reset or settled-receipt replay. Forged frame order
and live revocation fail closed. Storage authenticity, group-graph recovery and
external-effect crash atomicity remain separate host contracts.
The shared saved-body HIR entry also drives real reference
VM reentry; restoration hooks cannot replace its saved body or evaluator.
The shared flat-group kernel also checks complete original member order and
identities before native checkpoint commit; real VM admission and SQLite recovery
use it while native authority, reservations and dispatch/terminal state stay host-owned.
Shared recovery fences reject duplicate numeric reservations, check the original
allocation watermark before imported-identity repair, and gate completion receipts
before native admission. Exact numeric and GUI-local proofs run unchanged in the
consumer; real SQLite recovery uses the same fences before watermark repair/commit.
Shared numeric watermark requirements preserve inclusive ceilings, existing gaps
and exact write decisions. Live allocation/restore and SQLite allocation/repair
keep native identities, original reservation proof and commit-before-publish.
Shared inherited context agreement keeps native authority/revision observations
and exact deadline/output equality. Atomic successor, group-prefix and tail paths
retain their original fuel/schema/token checks around this phase.
Shared borrowed capture extension retains original named bindings and the current
result while allowing data aliases. Live successor admission and SQLite recovery
keep native authority, schema/budget succession and original projection policy.
Shared borrowed capture correspondence keeps group-first, capture-major original
scans, including late native errors after an earlier true match. Live capture
advancement and SQLite recovery keep original projection and authority policy.
Shared ordered outcome summaries keep native first-stop and aggregate-output
precedence, with fail-closed ownership and no settlement or receipt authority.
Live sequence inspection and SQLite bound recovery use the same production core.
Shared sequential-frontier inspection checks contiguous completion before native
prefix admission and blocked-dispatch evidence, excluding the active pending row.
Real SQLite preserves original phase/fault order and refuses without storage repair.
Shared adjacent-chain inspection retains original row order and move-only frames,
bounded before hooks and released before handoff. SQLite keeps native pair policy
and frozen result-decode-before-next-request fault order; no saved body is evaluated.
Receipt authenticity, raw-result correspondence, settlement and replay remain native.
They do not provide a production storage backend, crash-atomic
external-effect transaction or generic saved-graph interpreter.
Saved flat-All partial barriers also restore through PreparedParallelSession::restore
using the same ParallelReplyCore, with complete row/identity and mandatory fresh
host admission. Numeric/text models cover all eight three-member completion masks,
serialized missing-reply recovery, original fuel and settled replay without extra
preparation/dispatch. Full mixed-group recovery and process-crash acceptance
remain open. The identical saved_parallel proof runs in both workspaces.
Saved flat-Seq frontiers restore through PreparedSequenceSession::restore and
the original SequenceReplyCore, deriving the cursor from an accepted prefix,
optional awaiting member and unpublished suffix. Fresh snapshot and per-request
publication checks remain mandatory; original requests/prefix move without copies.
Numeric/text serialized models cover all seven valid frontiers with original fuel,
while twenty invalid arrangements reject before native admission. Mixed helper/group
continuation and process-crash recovery remain open. The same saved_sequence proof
is copied unchanged into the independent consumer.
Composed mixed recovery now runs 252 two-host frontier combinations unchanged
through the native pipeline: atomic, seq/all/repeat, captured successor and a
second group under three or more helper frames. Replaced controllers retain
original request inventory, identities and fuel; stale/revoked/closed hosts and
redirected suffix handles reject before tail work. Every fuel cutoff and exact
continuous-run fuel parity use the original walker. This is an in-memory
composition proof, not portable mixed snapshots or production crash recovery.
IR, captures, native inventory, bounded snapshot copies and settlement remain
host-owned; full extraction stays open.
Saved native operands now rebuild through PreparedCall::restore with complete
row/name/position/scalar bounds before mandatory host admission and original
domain checking. Names rebind to fresh schemas and scalar buffers move without
reevaluation, preparation, identity allocation or a new meter. The unchanged
saved_call consumer proof drops old request/schema/IR objects, then restores all
seven Seq frontiers for numeric and text hosts from its own bytes: three original
preparations and dispatches, preserved fuel and fresh publication authority. This
is a queued-operand codec model, not complete mixed snapshots or process-crash
acceptance; provenance, publication and settlement stay host-owned.
Already-assembled flat group values restore through restore_group_reply using
shared cold shape/row checks and mandatory whole-group native admission. Saved
Seq/All collectors rebind accepted rows to fresh IR; assembly is not charged
again. The unchanged saved_group proof covers two numeric/text byte-codec hosts,
both group kinds and three saved-budget boundaries including zero, after every
original graph/schema/receipt/meter object has been dropped. Original assembly
completion and budget require host authentication; ordinary completed receipts
still use charged assembly. Lookup rechecks live policy. This is not portable
mixed captures, a production backend or process-crash acceptance.
The saved_group consumer also composes byte-backed captures through the original
snapshot, group, stack and accepted-reply APIs. A host-provided scalar/result/group
scope and group alias suspend under two typed-IR return frames; fresh objects
resume after every original runtime/schema/IR/view/meter is dropped. Numeric/text
hosts and Seq/All groups keep uninterrupted result/fuel parity at every cutoff.
One rebuilt group is shared by aliases, but both returns and projections recheck
policy. Outer cold refusal blocks all capture materialization; wrong pending
identity preserves input for explicit retry. This fixed completed-capture model
does not combine partial groups, unpublished queues, production storage or crash
transactions. Full portable mixed-checkpoint and extraction gates remain open.
The saved-group consumer also joins byte-backed captures to an unfinished Seq at all seven frontiers for numeric/text hosts and captured Seq/All groups (28 combinations). Real call operands read the captured group before suspension, then only unpublished operands are restored from bytes. Old graphs, views, schemas, requests and meters are dropped. Accepted/awaiting work is not republished; exact original assembly/return fuel and fresh policy remain enforced. This sequential model is not source-compiled mixed recovery, a production journal or crash settlement; the parallel extension below separately covers partial All.

The captured partial-All extension reuses the same preparation, assembly and stacked returns across eight completion masks and six receipt orders for two native payload schemas and captured Seq/All groups (192 combinations). All original objects are dropped before byte restoration. The restored barrier creates no requests, restores no calls and dispatches nothing; wrong/duplicate receipts preserve input and ordered once-only output. Every assembly/return fuel cutoff matches live execution. Cancellation and scope closure discard partial or ready results without capture materialization or parent re-entry; parent/external cancellation remain host-owned. This is not concurrent race acceptance, source-compiled mixed recovery or process-crash settlement.

The source-compiled saved-group extension parses numeric/text source with an explicit type-only host prefix through compile_catalog_prepared_group_source, then runs its owned IR directly through the same adapters. It covers 220 Seq/All frontier/order combinations, both captured group kinds, two return frames and every assembly/return fuel cutoff. Bad source names/types, stale catalogs and unauthorized cold calls reject before runtime work; restoration rechecks current authority. Only the unpublished Seq suffix can dispatch, while restored All dispatches nothing. This is a bounded source-expression checkpoint model with a host-supplied captured graph, not helper-origin whole-program recovery, repeat/multiple captured graphs, production process-crash settlement or full extraction.

The helper-origin whole-program extension starts from an empty scope and uses compile_catalog_prepared_group_helper_program without an external prefix. Its source helper executes the first Seq/All, then the authenticated reply re-enters the original walker to construct scalar/atomic/group-alias captures before the second group. Eight program variants cover 220 checkpoint frontier/order combinations and every pending assembly/return fuel cutoff through shared checkpoint writers and restore adapters. Wrong initial ownership, revoked policy and denied original result domains retain the input and cannot start later work. Restore neither recompiles nor replays the helper; only unpublished Seq work dispatches, while restored All dispatches nothing. This is one bounded two-group program family, not arbitrary program checkpoints, repeat/multiple captured graphs, production persistence, cancellation/crash settlement or full extraction.

The bounded-repeat extension compiles repeat and seq with a repeat tail through the same empty-scope whole-program path. Twenty-four numeric/text program variants cover 332 additional frontier/order combinations and every pending assembly/return fuel cutoff. Expanded names and operation tags survive original-IR checkpoints; operation identity is distinct from per-member receipt identity and result domain. Same-operation wrong-member, foreign-domain and duplicate receipts preserve inputs and queue state, followed by successful original receipt recovery. Compilation bounds, current/cold authority and exact committed-byte admission remain checked. This is compile-time expansion into existing Sequence IR, not a new runtime loop or general durable VM. Multiple captured graphs, production persistence, cancellation/crash settlement and full extraction remain open.

The two-capture extension executes two source helpers before a third Seq/All pauses. Its bounded host-owned table retains two distinct completed graphs, two aliases per graph, scalar/atomic captures and two original return frames. Sixteen numeric/text program variants cover 440 frontier/order combinations and every pending assembly/return fuel cutoff using the same checkpoint writers and prepared collectors. Equal graph shapes do not merge native owners. Each graph rebuilds once, while cached aliases recheck live authority and domains; either captured domain can still be denied between return frames. Completed preparation and helper dispatches never replay. Bounded table checks reject cross-graph references, owners and budgets independently of exact committed bytes. This is a two-capture, three-group source fixture, not arbitrary graph storage, portable codecs, production persistence, cancellation/crash settlement or full extraction.

The pending-session restoration entry now installs host-decoded binding return frames into the original Vm or LocalVm lifecycle. Whole-forest structure precedes mandatory native admission of original identity, graph/sites/captures, current policy, saved fuel and nonterminal settlement. Success starts Awaiting without preparation, dispatch or meter charge; acceptance only makes Ready, and explicit polling resumes original returns on the same meter. Local restoration rejects closed scopes and revokes before retained cleanup on refusal/unwind; cancel and owner Drop reuse existing terminal fencing. Eight unchanged consumer regressions exercise real suspensions, successor handoff, receipt rejection, exact fuel and native cleanup/unwind. Native callbacks/Drop remain trusted. This is not a codec, cancellation journal, process-crash settlement or a complete durable owning VM, and external collectors still require explicit host cancellation.

The owned-recovery extension joins existing two-capture byte checkpoints to LocalVm::restore_pending and with_metered_host_outcome. Original Seq/All collectors assemble their result on the same owned Fuel used by later capture returns. Seven added regressions cover 440 numeric/text frontier/order combinations, every assembly/return fuel cutoff, 1760 cancellation/Drop cases, 660 live-policy refusals and metered state/cleanup boundaries; a compile-fail proof prevents the meter borrow escaping. Debits survive failure, cancellation and unwind without automatic refunds. Native callbacks are trusted and must not replace/refill Fuel or omit charges. Cancellation explicitly hands external collectors back after revocation, then the host cancels/disposes them. The bounded post-checkpoint bodies are pure; unexpected successors refuse rather than invent a declaration. This is not a production durable owner, general checkpoint codec, replay fence, external-effect rollback or crash settlement.

The shared dispatch-lease fence checks exact current attempt/retry/readiness/expiry observations before elapsed time, without allocation or storage. Reference in-memory and SQLite acknowledgement, retry and successor paths delegate to it. Expiry-time completion stays inclusive only for the current attempt; re-leasing fences old receipts. Both journals now reject completion before semantic retry readiness. Five unchanged consumer tests include a 1296-case matrix, full-width comparisons and opaque GUI-local identity observations. Request provenance, clock/counter policy, current authority, authoritative terminal replay and atomic settlement remain host-owned. This pure check is not a durable lease, replay registry or restoration journal.

The shared semantic retry planner reuses capped backoff and portable clock arithmetic to propose an exact next retry count and future ready time without payloads, timers or state. Exhaustion never refills the counter; permanent-error classification, policy ceilings and execution deadlines remain native. Both reference journals share proposed-schedule checks, closing the in-memory clock/count-ceiling gap and refusing counter overflow before mutation. Eight unchanged consumer tests cover 20720 planning and 1512 commit-check cases plus full-width and observation boundaries. Reference terminal replay still precedes malformed late metadata, and SQLite reopen preserves original leases and saved fuel. A proposal is not authenticated policy, a current lease, a retry registry or durable settlement.

Shared dispatch capacity distinguishes oversized batches from transient pressure with overflow-safe arithmetic and reuses the same active-slot predicate in both reference journals. Six unchanged consumer tests cover 729 admission and 49 slot combinations, full-width/zero boundaries and the original synchronous Admission lifecycle. Product policy ceilings, diagnostics, eligibility, clock expiry and transaction ownership remain native. SQLite keeps its bounded count, lowered limits do not revoke existing leases, and delayed work stays idle until ready. A successful observation is not a reservation, queue or atomic admission owner.

Shared dispatch issuance planning now reuses portable clock arithmetic with explicit positive-duration ceilings and exact overflow-safe delivery attempts. Six unchanged consumer tests cover 294 timing and 17161 counter combinations and original fencing. Both reference journals preserve fault codes, eligibility/backpressure order and native transaction ownership. A red/green regression fixes maximum-clock successor completion by separating existing-lease settlement from fictitious future-lease validation; replay, cancellation and saved fuel survive reopen. Attempt exhaustion does not revoke a current lease at its inclusive expiry. Numeric plans provide neither reservations nor execution authority or durable settlement.

Shared-owner graph restoration lends the original scope and native value with
ScopedLocalExecution::as_parts_mut. Raw Vm::restore_pending_in_scope and prepared
Seq/All::restore_in_scope use that same fence and existing restoration cores.
Parent and collector transfers retain original buffers, identities and fuel.
Refusal or unwind closes before retained restore-input cleanup; a restored parent
stays owned until explicit host cancellation. The unchanged coupled pipeline
checks every child restoration boundary on twelve numeric/text combinations and
every fuel cutoff; local owner borrowing tests also run in the separate consumer.
Arbitrary host mutation and callback-internal cleanup remain trusted. No owning
graph snapshot, durable codec, external cancellation or full extraction is added.

Forced publication termination now pauses the original fixture writer after a partial write, after file sync and after rename. Twenty-four owned-child kill chains cover both native hosts, pending Seq/All and cancellation intent/terminal publication; Unix runs verify signal 9. Sixteen orphan candidates block recovery, cancellation retry and stale-adapter dispatch without promotion or deletion. Only the parent, after reaping its sole writer in an exclusively owned test directory, discards the orphan before an explicit fresh cancel retry. Committed intent remains fenced and committed terminal survives a lost acknowledgement. Eight same-process file/directory collision cases also fail closed. This adds 112 child processes to the earlier 464-process slice. It changes test-host policy only: production VM contracts and format 5 are unchanged. There is no automatic cleanup, concurrent ownership protocol, power-loss guarantee or external-effect atomicity.

These are protocol tests, not actual GUI windows or native callback sandboxing.

`export-manifest.json` records relative paths, exact SHA-256 and source provenance.
`verified` is true only after isolated workspace/consumer tests and strict lints
pass and original inventory bytes remain unchanged. It is not a signature.

This is a source checkpoint, NOT the complete independent durable VM or full
repository extraction gate. Durable recovery, reference-adapter/wire/auth/GUI parity
and release publication remain separate gates in the original repository. Packages
are deliberately marked publish=false. FFI and an OS shell remain deferred.
