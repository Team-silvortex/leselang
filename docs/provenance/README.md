# Extraction Provenance

These files preserve the reviewed source-export and bootstrap snapshots from
before independent repository development. Their source versions, hashes,
acceptance totals and gate values describe those historical snapshots. Later
implementation and verification do not rewrite them.

| File | Historical scope |
| --- | --- |
| [Source checkpoint](source-checkpoint.json) | Export inventory, source identity and extraction gates |
| [Bootstrap verification](bootstrap-verification.json) | Verification receipt for the initial exported snapshot |
| [Extraction baseline](extraction-baseline.md) | Original native extraction acceptance and remaining gates |
| [Runtime-core baseline](runtime-core-baseline.md) | Source repository's runtime/ownership acceptance history |

The imported Markdown retains references to the source repository's paths.
In particular, the five `../../docs/leselang-embedding.md#...` links in the
runtime-core baseline refer to that source repository's embedding document;
that document was not exported here. These are preserved source references,
not working links to the current independent guide. They have not been redirected
to headings with different contracts or presented as repaired source evidence.

For current use, start with [Rust embedding](../embedding.md),
[architecture](../architecture.md) or the [documentation index](../README.md).
The [repository handoff](../migration.md) explains extraction ownership and
separate release gates. Later native desktop, JavaScript and agent results live
in [the acceptance archive](../acceptance/README.md).
