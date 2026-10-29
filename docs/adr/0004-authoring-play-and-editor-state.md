# ADR-0004: Authoring, Play, and editor state

Status: Accepted core direction for state lifetimes and prepared scene activation; apply-back and extended editor
policies remain proposed. The accepted core is implemented in the inspected tree; its verification limits are recorded
in the resolved page.

Date: 2026-10-27

Decision update: 2026-10-29, following explicit user acceptance of the researched recommendations.

Scope: State ownership, Play transitions, inspectors, and explicit runtime-to-authoring changes.

Dependencies: [ADR-0001](0001-editor-command-pipeline.md) supplies authored transactions and history;
[ADR-0002](0002-object-composition-and-components.md) supplies object/component identity;
[ADR-0003](0003-rust-typescript-bridge.md) supplies transport contracts. Coordinate timing with
[ADR-0005](0005-runtime-lifecycle-and-scheduling.md), storage with
[ADR-0011](0011-persistence-autosave-and-recovery.md), and asset overrides with
[ADR-0009](0009-prefabs-and-instance-overrides.md). Those proposed contracts remain dependencies, not existing features.

Authoritative accepted core: [Resolved decision](resolved/0004-state-lifetimes-and-prepared-activation.md). This
original URL retains research, history and remaining proposals; those extensions are not resolved by the core
implementation.

## Context and example

A designer places a player at the origin, presses Play, and walks across the scene. Meanwhile, the designer resizes
the inspector and collapses its Transform subject. Stopping must restore the authored player position while preserving
the chosen layout. A scene save must not accidentally serialize the player's runtime position or the collapsed card.
If the designer wants to keep a position found during Play, that needs an explicit, reviewable authoring operation.

## Existing implementation inspected

The current inspection baseline is `b6ec4ba5a88c5ef66eff82fa61eb5747b940400d`, plus the staged, unstaged, and
untracked tree inspected on 2026-10-29. The earlier drafting baseline was `fcb748b`; current working-tree behavior
was inspected again because these changes are not all represented by HEAD. Concurrent optimization work was preserved.

| Source                                                              | Observed behavior                                                                                                                                                                                                                                                                                                                                             |
|---------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/editor/mod.rs`                              | `EditorSession` stores authored and saved documents separately from `Option<GameSession>`. `render_document()` selects the active simulation document; `snapshot().document` remains authored data. Starting Play requires Edit mode and no transaction.                                                                                                      |
| `crates/engine-core/src/game_session.rs`                            | `GameSession` retains shared `Arc<ProjectData>`, a module factory and an active `SimulationSession`. Prepared scenes carry owner identity and expected scene generation. Commit preserves pause and clears input. It now owns bounded typed ProgressState; prepare_new_game constructs fresh defaults and commit rotates run identity after candidate checks. |
| `crates/engine-core/src/simulation_session.rs`                      | Scene-local transforms, collisions, input, clock and tick belong to `SimulationSession`. The shared-project construction path resolves behaviors and supplies validated `PreparedModuleConfig` to the factory.                                                                                                                                                |
| `apps/game/src/main.rs`, `crates/runtime/src/scene_editor.rs`       | Player-mode F8 prepares CPU and rendering candidates, waits for GPU work, commits the scene and installs its view. Held input and native scene handles are cleared. Development-mode restoration across runtime scene switches remains outside this feature.                                                                                                  |
| `crates/engine-core/src/editor/scene_documents.rs`                  | `DocumentCostCache` accounts for retained authored snapshots. It is editor infrastructure, separate from gameplay progress.                                                                                                                                                                                                                                   |
| `apps/editor/src/app/shared/services/layout-preferences.service.ts` | Defaults are merged with decoded saved component configuration. Changes use a debounced, serialized persistence queue with error/dirty state.                                                                                                                                                                                                                 |
| `apps/editor-host/src-tauri/src/layout_preferences.rs`              | Layout preferences have a separate schema and persistence path from scene/project data.                                                                                                                                                                                                                                                                       |

Selection and snap settings currently belong to the backend editor session. Consequently, “editor state” does not mean
“everything lives in Angular.” Runtime inspection also needs an explicit read model: the existing authored snapshot is
not a live runtime-object snapshot.

## Accepted core decisions

### Ownership and research rationale

Use explicit ownership at each lifetime. Unreal's `UGameInstance` spans a running game and its subsystem model
distinguishes game-instance, world, and editor lifetimes. Unity preserves selected objects across scene loads through
`DontDestroyOnLoad`; Godot Autoload nodes can retain information between scenes. These are precedents for a longer-lived
owner, not a requirement to copy their inheritance or global-access mechanisms.
[Unreal GameInstance](https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/Engine/UGameInstance),
[Unity scene persistence](https://docs.unity.com/en-us/engine/6000.0/script-reference/unityengine/object/dontdestroyonload),
[Godot Autoload](https://docs.godotengine.org/en/stable/tutorials/scripting/singletons_autoload.html).

Our chosen game-run lifetime ends on Stop or an explicit New Game/reset, even when the application remains open.
Immutable authored settings may be shared through `Arc<ProjectData>`. Mutable progress belongs to the game run and
does not alias definitions. Typed game state and its domain operations follow ADR-0008; a universal string-keyed
integer/flag map is not the primary gameplay model.

### Runtime lifetime and reset policy

| Operation                                 | Game-run progress                | Active scene state                                                    |
|-------------------------------------------|----------------------------------|-----------------------------------------------------------------------|
| Start Play or New Game/reset              | Initialize game-defined defaults | Construct a fresh scene instance                                      |
| Pause/resume                              | Preserve                         | Preserve world; neutralize gameplay input at pause                    |
| Successful scene transition               | Preserve                         | Replace with the prepared scene; begin with neutral input             |
| Failed preparation or rejected candidate  | Preserve                         | Preserve the installed scene                                          |
| Stop                                      | Discard                          | Dispose after owned work is quiescent; editor restores authored state |
| Reload that constructs a new game session | Initialize defaults              | Construct a fresh scene instance                                      |

These are accepted semantics now implemented in the native path: ordinary transitions retain progress and prepared New
Game resets it. The current reload path constructs a new session. A future reload that preserves progress requires an
explicit migration
policy. Disk game saves have a separate contract from in-memory lifetime and editor persistence.

### Prepared scene activation

Separate loading, validation and construction from activation. A candidate may coexist in memory with the installed
scene, but it does not tick, mutate game-run progress, award rewards or publish gameplay events. Factories must respect
that preparation contract; immutable Rust borrows alone cannot prevent arbitrary interior mutation or external effects.

At a host/simulation boundary, finish required preparation, quiesce outgoing rendering work, check candidate owner and
generation, replace the active simulation and install its corresponding rendered scene before the next update/frame.
Only the active scene executes gameplay. Preparation failure keeps the previous scene installed. The initial native
path is synchronous; nonblocking/background loading is not implied. Temporary memory for the outgoing and candidate
scenes must be budgeted before supporting larger content.

Unity's delayed activation and Godot's deferred scene replacement support separating preparation from active execution;
their exact ordering and failure guarantees differ from this engine's chosen contract.
[Unity activation control](https://docs.unity.com/en-us/engine/6000.0/script-reference/unityengine/asyncoperation/allowsceneactivation),
[Godot SceneTree](https://docs.godotengine.org/en/stable/classes/class_scenetree.html).

This decision replaces the earlier draft requirement to stop a runtime before constructing any replacement. Stop is
required before the replacement begins gameplay; inactive candidate construction is allowed first.

## State domains and remaining proposals

The ownership and activation rules above are accepted. The following state-domain model elaborates that direction;
apply-back, preference extensions and future read-model APIs remain proposals until separately agreed and implemented.

### Three state domains

| Domain   | Authority and contents                                                                                                                     | Persistence and history                                                                                                               |
|----------|--------------------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------|
| Authored | Rust authoring documents: object/component definitions, hierarchy, graph parameters, asset references, project settings.                   | Explicit save/recovery policies; authored commands participate in undo/redo.                                                          |
| Runtime  | Game-run progress owned by `GameSession`, plus scene-local handles, transforms, input and systems owned by its active `SimulationSession`. | Game progress survives scene transitions; scene-local state resets. Both are disposable on Stop. Game saves have a separate contract. |
| Editor   | Selection, editing tools, inspector drafts, camera navigation, pane sizes, theme overrides, and workspace preferences.                     | Persist only designated preferences; never enter game builds or scene undo history.                                                   |

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
replacement follows the accepted prepared-activation boundary above; candidate construction can precede disposal of
the outgoing simulation. Neither pause nor stop implicitly saves authored content.

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

Typed game-owned progress and aggregate staged updates are now present; the resolved pages specify the bounded native
interface and its trust limits. Preserve shared project ownership and editor document-cost
caching. Do not reintroduce per-update full project/scene copies to implement progress.

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
- Scene switches preserve typed game progress, clear old input and reject stale or foreign prepared candidates;
  preparation produces no progress changes or gameplay events. Stop/New Game establishes a fresh run identity.
- When apply-back is introduced, test selection, conflict rejection, prefab overrides, and one-step authored undo.

These are implementation gates. This 2026-10-29 decision update inspected history and the working tree, including
optimization changes. Documentation publication checks are recorded in `docs-site/docs-state.json`; they do not verify
native Play, persistence failures or every memory-budget case. Typed progress/New Game are present; newer attributed
test evidence and snapshot limits are in the resolved page.
