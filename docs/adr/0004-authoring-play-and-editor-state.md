# ADR-0004: Authoring, Play, and editor state

Status: Proposed; this ADR does not implement new Play or persistence behavior.

Date: 2026-10-27

Scope: State ownership, Play transitions, inspectors, and explicit runtime-to-authoring changes.

Dependencies: [ADR-0001](0001-editor-command-pipeline.md) supplies authored transactions and history;
[ADR-0002](0002-object-composition-and-components.md) supplies object/component identity;
[ADR-0003](0003-rust-typescript-bridge.md) supplies transport contracts. Coordinate timing with
[ADR-0005](0005-runtime-lifecycle-and-scheduling.md), storage with
[ADR-0011](0011-persistence-autosave-and-recovery.md), and asset overrides with
[ADR-0009](0009-prefabs-and-instance-overrides.md). Those proposed contracts remain dependencies, not existing features.

## Context and example

A designer places a player at the origin, presses Play, and walks across the scene. Meanwhile, the designer resizes
the inspector and collapses its Transform subject. Stopping must restore the authored player position while preserving
the chosen layout. A scene save must not accidentally serialize the player's runtime position or the collapsed card.
If the designer wants to keep a position found during Play, that needs an explicit, reviewable authoring operation.

## Existing implementation inspected

The inspected baseline is `fcb748b04853209b5e17e680c38c847c19f7f365`, plus staged, unstaged, and untracked changes
on the date above. Concurrent changes to simulation scene targeting are work in progress and were preserved.

| Source                                                              | Observed behavior                                                                                                                                                                                                                |
|---------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/editor/mod.rs`                              | `EditorSession` stores authored and saved documents separately from an optional `SimulationSession`. `render_document()` selects the Play world when present; `snapshot().document` remains authored data.                       |
| `crates/engine-core/src/editor/mod.rs`                              | Starting Play requires Edit mode and no open transaction. The inspected uncommitted caller passes the active scene explicitly to `with_factory_for_scene`. Stopping drops that runtime. Authored mutations reject while playing. |
| `crates/engine-core/src/simulation_session.rs`                      | Runtime transforms, input, pause state, and tick are session-owned. The uncommitted factory changes make scene targeting explicit; they are not treated as verified implementation.                                              |
| `apps/editor/src/app/shared/services/layout-preferences.service.ts` | Defaults are merged with decoded saved component configuration. Changes use a debounced, serialized persistence queue with error/dirty state.                                                                                    |
| `apps/editor-host/src-tauri/src/layout_preferences.rs`              | Layout preferences have a separate schema and persistence path from scene/project data.                                                                                                                                          |

Selection and snap settings currently belong to the backend editor session. Consequently, “editor state” does not mean
“everything lives in Angular.” Runtime inspection also needs an explicit read model: the existing authored snapshot is
not a live runtime-object snapshot.

## Proposed decision

### Three state domains

| Domain   | Authority and contents                                                                                                                 | Persistence and history                                                             |
|----------|----------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------|
| Authored | Rust authoring documents: object/component definitions, hierarchy, graph parameters, asset references, project settings.               | Explicit save/recovery policies; authored commands participate in undo/redo.        |
| Runtime  | A particular Play/player world: resolved handles, live transforms, controller state, animation time, physics state, transient objects. | Disposable on Stop. Future game saves require their own contract.                   |
| Editor   | Selection, editing tools, inspector drafts, camera navigation, pane sizes, theme overrides, and workspace preferences.                 | Persist only designated preferences; never enter game builds or scene undo history. |

Shared immutable asset data may be referenced by both authored and runtime state. Mutable runtime instances must not
alias authored definitions. World/session identity distinguishes the same authored object instantiated in different
runs.
Object identity alone is insufficient for targeting a runtime request.

### Start, pause, and stop

Start Play is an ordered barrier after preceding authored commands. Resolve an active drag or form transaction
explicitly
through its normal commit/cancel policy; do not invent a final value from invalid text. If a draft cannot be committed,
surface the affected field and leave the start request unfulfilled. No half-applied transaction becomes the runtime
seed.

Create Play from a validated snapshot of the selected scene and required project data, including unsaved authored edits.
Capture its source revision and assign a fresh runtime generation. Saving first is not a requirement. Asset resolution
or initialization failure leaves the authored session usable and reports a failed start; it must not produce a partially
active world. Asset-version pinning follows [ADR-0006](0006-asset-identity-and-import-pipeline.md).

Initially retain the current restriction on authored content edits during Play. Runtime controls and runtime inspection
are separate operations with clear mode labels. Pausing retains the runtime but neutralizes gameplay input. Stopping
quiesces runtime work, invalidates its handles/subscriptions, discards it, and shows authored state again. Scene
replacement
must stop the old runtime before constructing another. Neither pause nor stop implicitly saves authored content.

Save and autosave serialize authored state only. A runtime tick does not make the authored document dirty. Keep command
revision, document revision, runtime generation, and simulation tick distinct as specified by ADR-0003. A runtime debug
view can show changing values without pretending they are pending authored edits.

### Explicit apply-back, later

Do not initially offer unrestricted “keep all Play changes.” A future **Apply selected runtime changes** operation must:

1. Pause and capture a candidate diff against the runtime's source snapshot, with source identities and runtime
   generation.
2. Show allowlisted authored fields and their before/after values. An initial proof may support existing-object
   transforms
   only; this ADR does not declare that proof complete.
3. Stop Play, validate targets and source revisions, and apply the selected values as one authored transaction.
4. Reject conflicts or missing targets with a precise result, retaining the candidate for review without silently
   merging.

Runtime handles, graph playback state, velocities, transient caches, and subscriptions are never copied into authoring
documents. Runtime-created objects need an explicit conversion/import design before inclusion. Prefab-derived properties
must create valid instance overrides through ADR-0009. Undo restores the previous authored values; applying does not
save.

### Preferences and drafts

Initialize each configurable component from versioned defaults plus validated saved overrides. Missing or invalid
entries
fall back to defaults with a recoverable diagnostic. Reset removes the relevant overrides so future defaults can take
effect. Keep defaults immutable and avoid writing them merely because a component mounted.

Use stable preference keys and explicit scopes. Global appearance and a project's workspace layout may have different
scopes; storage locations and portability follow ADR-0011. Unsaved inspector text is a draft, not either a user
preference
or an accepted object value. An editor reload must not claim a draft is durable unless a recovery feature actually saved
it.

## Alternatives and tradeoffs

Editing the authored document directly during Play saves initial integration work but makes undo, autosave, and runtime
side effects inseparable. Copying the entire runtime back on Stop similarly confuses generated and intentional changes.
Both are rejected. Concurrent authored editing with live runtime patching remains possible later, but needs a specified
patch/restart policy and conflict model first.

Separate worlds consume memory and require inspectors to identify their source. Shared immutable resources and scoped
runtime queries limit that cost without weakening ownership. Keeping layout outside scene history means layout changes
need their own reset behavior, not scene Undo.

## Open choices and implementation sequence

Open choices are the first apply-back field allowlist, runtime inspection refresh budgets, preference portability, and
whether future live editing restarts affected systems or patches them. They do not block establishing state boundaries
now.

Implement explicit state-domain/read-model contracts first, then Start/Stop barriers and mode-aware inspectors, then
preference reset/migration. Add apply-back only after object identity, transaction conflicts, and prefab overrides have
working proofs. Runtime game-save support remains separate from editor recovery.

## Acceptance and verification limits

- Start from unsaved authored edits; move in Play; Stop restores authored values and preserves the original dirty state.
- Save/recovery output excludes runtime motion and UI preferences. Layout changes survive independently where
  configured.
- Invalid drafts and open transactions cannot produce a half-committed Play seed.
- Stale requests from a stopped runtime cannot mutate its replacement, even when object identities match.
- A failed start cleans up partial runtime resources. Pause, focus loss, and Stop cannot leave movement held.
- When apply-back is introduced, test selection, conflict rejection, prefab overrides, and one-step authored undo.

These are implementation gates. This documentation pass inspected source and the working-tree diff; it did not run
native
Play, persistence-failure, or performance acceptance tests.
