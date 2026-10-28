# Security Boundaries

Leselang executes against an explicitly implemented native host. Host callbacks,
destructors, clocks, storage and dispatch are trusted code, not sandboxed plugins.
Source/IR/fuel limits do not preempt arbitrary native callbacks. Scope revocation
is cooperative and cannot undo an already committed effect or survive every
process-abort scenario. Serialization alone does not authenticate a checkpoint.

Adapters must enforce current grants, authentic request/reply identity, bounded
input and native work, framework thread affinity, and durable effect settlement
when required. Do not execute untrusted scripts against an unrestricted host.

When reporting an issue, include the crate revision, host assumptions, bounded
reproduction and expected/actual outcome. Remove credentials and private source
data. Use a private advisory/contact channel offered by the repository maintainer
for exploitable issues; do not post secrets in public issues. No dedicated
reporting address or response-time commitment is established by this document.
