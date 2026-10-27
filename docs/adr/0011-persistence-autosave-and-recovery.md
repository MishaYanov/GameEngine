# ADR-0011: Document saves, autosave recovery, and external changes

Status: Proposed; no autosave, multi-document commit protocol, or additional durability guarantee is implemented here.

Date: 2026-10-27

Scope: Authored project documents, explicit saves, recovery checkpoints, dirty baselines, and filesystem conflicts.

## Question and example

A user edits two scenes and a prefab, changes the catalog, then loses power during Save All. Which files reached
disk, which drafts remain recoverable, and can reopening combine an old manifest with missing assets? Another editor
or version-control checkout can also change files while this editor is open.

Recommend separate explicit authored saves and automatic recovery checkpoints. Retain per-document replacement,
report partial Save All outcomes, and detect external modifications. Do not call several file replacements a single
atomic transaction. Stronger whole-project publication requires a separate generation-based storage design.

## Existing implementation inspected

At `fcb748b04853209b5e17e680c38c847c19f7f365` and the inspected working tree:

| Path/operation                                                      | Current behavior                                                                                                                                                                            |
|---------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/runtime/src/scene_io.rs`                                    | Serializes first, exclusively creates a sibling temporary file, writes, calls `sync_all`, closes, then renames over the destination. It does not first delete the old file.                 |
| `apps/editor-host/src-tauri/src/editor.rs`: scene Save              | Rejects an open grouped transaction, writes the active authored scene, and marks its baseline saved. It can save authored data during Play; simulated state is not substituted.             |
| Same file: Save As/project Save                                     | Require Edit and no grouped transaction. Save As writes a new destination, changes its catalog path, and leaves the old file. Project Save writes the manifest without saving dirty scenes. |
| Same file: `atomic_json`                                            | Uses temporary write/sync/close/rename for project/prefab JSON. Its name alone does not establish power-loss durability.                                                                    |
| `compiler/src/lib.rs`                                               | Captures/checks saved build inputs and rejects detected changes; this is not an immutable filesystem snapshot.                                                                              |
| `apps/editor/src/app/shared/services/layout-preferences.service.ts` | Debounces and serializes layout preference writes; it is not authored-scene autosave.                                                                                                       |

Save As can leave a new unreferenced file if its later catalog commit fails. The project and scene writes are
separate. These paths do not provide general document recovery checkpoints or a multi-writer conflict protocol.
See [project lifecycle](../project-lifecycle.md) and [scene persistence](../scene-persistence.md).

## Dependencies and terminology

[ADR-0001](0001-editor-command-pipeline.md) supplies the authoritative mutation queue and revisions.
[Authoring/Play separation](0004-authoring-play-and-editor-state.md) defines what may persist.
[Asset metadata](0006-asset-identity-and-import-pipeline.md), [graph assets](0007-graph-assets-and-execution.md), and
[prefab operations](0009-prefabs-and-instance-overrides.md) expand the document set; they must share save semantics.

An authored document is intentional project content. A saved baseline identifies the exact revision/content last
written successfully. A recovery checkpoint preserves committed authoring drafts outside normal project files.
User preferences, renderer caches, runtime state, and unfinished form text belong to separate stores. A transport
receipt or command journal is not a durable edit log and cannot replace document recovery.

## Proposed save contract

Capture a validated immutable document snapshot and its revision through the command queue. A save job carries
workspace generation, document ID, expected disk fingerprint, and intended new fingerprint. Write outside the
mutation-critical section, then reconcile completion through the same queue. Only that captured content becomes
the saved baseline; edits made during I/O remain dirty. A completion from a replaced workspace cannot mark its
replacement clean.

Use a shared persistence adapter for scenes, manifests, prefab/graph sources, and sidecars. Serialize and validate
before touching the destination. Retain unique sibling temporary files and failure cleanup; never erase the old
file as a replacement fallback. New destinations require an exclusive publication strategy, not just an earlier
existence check. Successful completion must distinguish written, failed, conflict, and outcome-uncertain states.

Rust documents `sync_all` as an attempt to synchronize file content/metadata, while `rename` has platform-specific
behavior and cannot cross mount points. Those APIs alone do not promise survival of every power failure, persistence
of directory changes, or cooperation by other writers. Define and fault-test the actual supported Windows/local
filesystem guarantees before strengthening user-facing
claims. [Rust File::sync_all](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all),
[Rust fs::rename](https://doc.rust-lang.org/std/fs/fn.rename.html).

Preserve current ordinary Save behavior during Play: capture only authored state. Save As, catalog changes, and
Save All initially require Edit with no active grouped transaction. Recovery skips unfinished transactions and
captures the latest committed authoring state after commit/cancel. No save operation silently accepts simulated
values or unvalidated form drafts.

## Save All and partial failure

Build a bounded save plan containing document identities, captured revisions, target paths, hashes, and dependency
order. Validate the entire plan first. Persist newly referenced documents before publishing the manifest that
references them. Track each replacement independently; existing referenced files can already have changed if a
later write fails. Manifest-last ordering reduces dangling references but does not provide all-or-nothing behavior.

Return per-document results and preserve dirty baselines for unsuccessful writes. Keep a bounded recovery record
of the plan and observed completions, reconciling disk hashes after an interruption; an absent completion record is
not proof that a write failed. Never roll back successful files blindly over subsequent external edits. Offer
review/retry from the retained draft or recovery checkpoint. Build remains blocked until its required saved inputs
are valid and consistent with the requested authoring state.

An immutable project-generation directory plus a published generation pointer could provide a clearer whole-project
commit boundary, but changes ordinary file layout, source-control workflows, and garbage collection. Defer it until
multi-file consistency requirements justify that cost; do not emulate it with undocumented rename ordering.

## Autosave and recovery

Automatically create recovery checkpoints after committed changes, with a debounce plus maximum interval so steady
editing cannot postpone them indefinitely. Store them in an ignored recovery location scoped to a stable project
identity and workspace lineage. Persist that identity explicitly; a directory path alone is not enough after a move.

Each checkpoint has a format version, schema versions, document IDs, revisions, original paths, saved-base hashes,
content digests, and complete bounded document snapshots. Publish its manifest after its payloads. Only checkpoints
whose manifest and referenced payloads validate are offered for recovery; preserve the previous valid checkpoint
until a replacement is complete. Derived caches and Play state are excluded. Incomplete checkpoints are quarantined
or cleaned using bounded retention rules, never loaded as partly valid scenes.

Opening a project compares recoverable drafts with current disk content. Show changed documents and timestamps;
restore into new unsaved authoring drafts after validation/migration. Do not overwrite current files automatically.
If disk content diverged from the checkpoint's saved base, expose a conflict with recover, keep-disk, and save-copy
choices. Normal undo history need not survive a restart; recovery establishes a documented new history boundary.

## External changes and tradeoffs

Store content fingerprints at open/save and recheck before replacement. File watchers and timestamps are hints to
refresh status, not proof of unchanged bytes. A changed fingerprint blocks automatic overwrite. Clean documents
may offer reload; dirty documents require explicit conflict resolution. Preserve unknown future-schema files rather
than rewriting them with a guessed interpretation.

This check has a race between comparison and replacement. A project writer lock coordinates participating editor
instances, but cannot guarantee exclusion of arbitrary tools. Therefore the first release documents single-writer
operation, keeps recovery copies, and reports detected conflicts without claiming universal compare-and-swap.

Saving every keystroke into source files is simple but merges recovery with intentional publication and exposes
incomplete edits. Durable event sourcing could reconstruct history but requires stable replay semantics across
schema changes. Bounded snapshot recovery is larger yet simpler to validate and evolve.

## Implementation and acceptance gates

Implement the persistence adapter and captured-revision baseline tests first, then external-change detection,
per-document Save All reporting, and bounded recovery snapshots. Choose checkpoint cadence, quotas, backup retention,
lock behavior, and the supported filesystem matrix before release.

Inject failures before/after write, sync, replacement, completion recording, and manifest publication. Test full disks,
permission errors, external edits, stale workspace completions, new edits during save, corrupt/incomplete recovery,
schema migration, and Play saving only authored content. Verify unchanged files survive rejected writes, partial Save
All results stay visible, and restoring never overwrites disk implicitly. Process-crash tests do not establish
power-loss durability; that requires a separately documented platform test. No such implementation or durability
acceptance is claimed here.
