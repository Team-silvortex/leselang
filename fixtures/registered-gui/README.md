# Registered GUI publication fixture

[publication.json](publication.json) describes an article editor with two
registered controls, `draft` (editable text) and `publish` (button). Both
start with an empty value and zero clicks. An ordinary edit stores the
supplied text. An ordinary click copies the current draft to the published
value and increments the click count. Each Consumer implements these effects
on its own application state.

## Running a case

1. Create a fresh article. Load the three Lese source files relative to the
   fixture and pass `arguments` to the publication program.
2. Apply the named fault at the boundary below. Record the primary request
   sequence, including bootstrap discovery and the final verification reads.
   Each token names the operation and, where applicable, its control.
3. Capture the terminal outcome and the application's committed edit and click
   counts. Keep the application alive. Run `read_draft` and `read_publication`
   in two fresh, read-only sessions with no injected fault. Each must return
   a string after two Effects. Compare these values with the actual application
   state and confirm that the counters stayed fixed across the reads.
4. Compare the resulting record to `expected`. A completed program supplies
   `result`, `effects` and `report_after`; `result` can be false. Failure and
   cancellation have null values for all three fields. A consumer marks
   `verified` true only when the program returned true, its final observations
   and the application agree on both requested values, and exactly one edit
   and one click committed. `commits` counts writes made by the program.
   External edits affect subsequent reads.

| Fault | Action in the primary session |
| --- | --- |
| `none` | Execute the publication as requested. |
| `acknowledge_without_edit` | Acknowledge the draft write while leaving its value and commit count untouched. |
| `wrong_publish_target` | After editing, return the draft target for a lookup of the publish button. |
| `unversioned_edit` | Commit the edit, then return the new draft with its old revision. |
| `reject_click` | Reject the click before publishing. |
| `unacknowledged_click` | Commit the click, then return a false acknowledgement. |
| `final_read_invalid` | After the program observed the publication, return a foreign identifier in the host's final draft read. |
| `publication_superseded` | During that final draft read, update the published value to `superseding_text` and advance its revision. |
| `disabled_draft`, `readonly_draft` | Initialize the draft as disabled or non-editable and let the program inspect it. |
| `cancel_before_edit`, `cancel_after_edit` | On receipt of the first edit, retire the primary binding before committing or immediately after committing, before reply acceptance. |
| `replaced_draft_at_write` | On receipt of the edit, replace the control under the same name. Check the incoming target against the new generation. |
| `external_edit_at_write` | At the same point, an outside editor stores `superseding_text` and advances the revision. Check the incoming write against the new revision. |
| `write_grant_revoked` | Withdraw write permission on receipt of the edit and check that permission before dispatch to the application. |

The fresh sessions resolve each control again. A host may choose its own
generations, revisions, session identifiers and diagnostic text.
The records compare the portable observations. Rust exercises the embedded
registered host. Node and C# each serve the registered GUI worker protocol
from their own process.

With the repository's pinned dependencies installed, run the three Consumers
and compare their per-case records:

```sh
sh scripts/check.sh registered-gui
```

Each run prints the directory containing the three JSON observation files.
The existing Rust tests also exercise the local dispatch/pump/accept
interleavings and a write exactly on gateway request 256. Those checks use
the direct embedding and gateway interfaces. [Local VM tests](../../hosts/registered/tests/local_gui_session.rs)
· [Gateway boundary tests](../../hosts/registered/tests/registered_gui_workflow.rs)
