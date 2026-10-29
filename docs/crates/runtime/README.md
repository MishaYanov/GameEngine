# Shared runtime and editor logic

Source: `crates/runtime`. CPU documents, commands and simulation live in engine-core and are re-exported here. Runtime
supplies shared asset/model/GPU-scene adapters and GraphicsProbe; Angular owns presentation and bindings.

## EditorSession: the Angular domain boundary

`runtime::editor::EditorSession` owns schema3 authored/saved data, persistent-key multi-selection, snapping, a temporary
Play copy, revisions, transactions, and bounded patch history. `execute(EditorCommandRequest)` validates
expected_revision and returns an authoritative bridge snapshot. `CommandFactory::capabilities()` advertises
intent/categories/undoability/Play availability without assigning physical keys.

Operations cover camera frame/orbit/pan/dolly, local/world/group transforms, hierarchy parenting, identity-preserving
edits, prefab export/instantiate, tint/light/collider changes, undo/redo and Play input/pause. `tick` uses a bounded
fixed step and updates scene_revision without turning every rendered frame into a command revision conflict. Saving uses
authored content and rejects an uncommitted transaction.

The CPU history is capped at 128 records / 16 MiB, selection at 4096. Atomic validation/error behavior matters as much
as nominal command success. This is separate from the older GPU SceneEditor history; do not reuse one session's
transient IDs in another.

## Project documents

Project schema 3 retains SceneCatalog/SceneId, startup_scene_id and scene-scoped behavior entries, and adds shared
behavior definitions. The JSON loader upgrades inline-only schema-2 manifests; serialization writes schema 3. Core
EditorSession
retains active/inactive drafts, per-scene saved baselines and scene-aware history; EngineBuilder::with_scenes prepares
all documents. Editor protocol 2 snapshots expose the catalog and dirty states.
See [project lifecycle](../../project-lifecycle.md) for caps, commands, version rejection, host paths and save/reload
semantics.

## Simulation scene context

`runtime::simulation_session` re-exports the CPU simulation API. `SimulationSession::new` and `with_factory` select the
project's startup scene. Use `with_factory_for_scene(project, scene_id, document, factory)` to construct a session for
another catalog scene. `scene_id()` reports that running scene; `project()` retains the original startup setting and
other project configuration. The selected scene document is validated before the factory is called.

Custom factories must implement the explicit scene argument:

```rust
pub trait GameModuleFactory: Send + Sync {
	fn create(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
	) -> Result<Box<dyn GameModule>, String>;
}
```

`ConfiguredBehaviorFactory` checks catalog membership and resolves inline/shared settings for the supplied ID.
Movement-input
validation uses the session's running scene ID as well. Editor Play supplies its active scene explicitly, so playing a
nonstartup scene does not repurpose the project's launch setting. Scene identity remains fixed for that simulation
session; [project lifecycle](../../project-lifecycle.md#editor-play-and-simulation-scene-identity) describes the
authoring
and startup distinction.

Editor Play now owns a `GameSession` containing the active SimulationSession. `prepare_play_scene` and
`commit_play_scene` allow explicit replacement with a validated candidate identified by session owner and generation.
Preparation leaves the active simulation installed. Commit preserves current pause state, clears movement and advances
editor revisions. The native SceneEditor adapter also resets its movement target, selection and retained resource
snapshots, and suppresses held input until release. Its caller must finish GPU work and prepare a matching rendered
scene before committing. The [Player transition path](../../apps/game/README.md#player-scene-switching) follows this
ordering; Angular has no transition command/control added by these APIs. Authored data remains separate from the
temporary runtime, and an outgoing simulation is not retained for later resumption.

## Game progress

`game_progress` is re-exported from engine-core. `GameSession` owns one game-defined progress value for a run, separate
from authored documents and the active simulation world. Ordinary scene replacement preserves its latest value;
New Game resets it and invalidates old prepared scenes. Starting/stopping editor Play creates/drops a run. No disk
save format, inventory schema, cross-process transfer or Angular progress editor is provided.

Implement `GameProgress` with an independent `Clone`, equality, validation, owned-byte accounting and an action type.
`ProgressState` exposes typed read-only `ProgressView::get` and a revision; changes pass through `ProgressAction` and
`apply_batch`, cloning a candidate only when an action is attempted. A rejected action/type/validation/budget leaves
the committed value unchanged. Changed commits advance the revision once; no-op candidates leave it alone. Limits are
64 KiB of reported retained progress, 4 KiB per action and 64 actions per batch/tick. Native game code supplies those
counts and must avoid external side effects in clone/validation/application; this is a trusted native contract, not
a sandbox or an independently measured heap quota.

`GameModuleFactory::create_progress` defaults to empty progress. `GameModule::needs_tick_with_progress` and
`plan_with_progress(GameplayContext, ActionWriter)` default to their earlier methods, preserving existing factories.
The context exposes world, input, fixed-step duration and a read-only progress view. Emit typed changes with
`ActionWriter::push_progress`. `GameSession::tick` runs world and progress actions together; later catch-up steps see
the candidate progress. A failed tick discards progress, restores touched world transforms through the failure path,
pauses simulation and clears movement. `TickOutcome` reports progress_changed/progress_revision separately from scene
changes. Use GameSession's tick/needs_tick APIs when progress matters; standalone SimulationSession methods supply an
empty progress value.

Factories can override `create_prepared(PreparedModuleConfig)` to reuse engine-validated scene identity and resolved
behaviors; its default delegates to `create`. `ValidatedScene` similarly borrows an immutable validated document so
downstream consumers can avoid repeating full validation without accepting arbitrary unchecked scenes.

## Synchronization and allocation reuse

EditorSession caches immutable retained-document costs, history scene-ID membership and lightweight state comparisons.
Selected transform/light/entity edits avoid redundant resolution/validation and stage replacements before mutating
authored state. Structural operations keep full validation/fallback paths. These changes preserve budgets, undo,
dirty-state and atomic-error contracts; they do not remove all document copies or establish measured latency gains.

`Scene::synchronize_authored(validated_document, changes)` applies a complete dirty set from the same authored revision,
updating changed metadata/transforms and affected descendant matrices. Parent/asset/topology changes return false for
full synchronization. Lost events or document replacement require full resync; a partial dirty set is not a valid
caller contract. The host uses this path for compatible Edit changes and `synchronize_world` for Play changes.

AssetManager batches missing model primitives/textures within renderer upload limits and caches imported source
resources. Logical GPU accounting still counts externally retained allocations until release. `GizmoPose::from_scene`
uses the entity's world position and parent transform; parent-aware segments/hit testing/drags convert the resulting
world gesture back into local authored transforms. The native tools now use this path for parented objects.

The simulation caches distinct resolved controlled-movement entity keys for its selected scene at construction.
`controlled_targets()` exposes that fixed list; `EditorSession::play_controlled_targets()` and the GPU SceneEditor
adapter expose it while Play is active. `set_controlled_input` supplies normalized movement to all those targets;
`set_input` requires an individual target on that list. SceneEditor also provides `set_play_input` and
`set_entity_play_input` adapters. These helpers preserve the existing input-transition clock reset behavior.

`MovementTarget::{AllControlled, Entity(key)}` routes normalized movement through `set_targeted_input`. SceneEditor
starts Play with `AllControlled`; `set_play_input_target(target, input)` validates a selected controlled entity, clears
old movement and suppresses held buttons until release. Replacing the project or starting/stopping Play resets the
target. `set_play_movement_bindings(bindings, input)` likewise clears old movement and suppresses held buttons when
the mapping changes. This is explicit movement routing, not a general possession or player-controller system.

## Physical input and movement profiles

`input::InputState` tracks keyboard and mouse buttons via `InputButton::{Key, Mouse}`. `press`, `release`, `is_held`,
`just_pressed` and `just_released` accept either button type. Repeated presses keep one pressed edge; `end_frame`
clears edges, `suppress_held` prevents held buttons from rearming until release, and `reset` clears all input state.
`Key` identifies physical positions, including named international/media/function keys and platform-tagged
`NativeKeyCode` values for unidentified keys. Mouse buttons include left/right/middle/back/forward and numeric extras.
These types are independent of winit and do not represent typed text, IME input, cursor positions or analog axes.

`input::MovementBindings` provides normalized two-axis held-key input with `WASD` and `ARROWS` presets. Camera movement
uses arrows; SceneEditor's Play input defaults to WASD and accepts a replacement mapping. `InputProfile` serializes
schema version 1 and four keyboard movement bindings as bounded JSON, rejecting unknown fields, unsupported versions
and input over 4 KiB. The native game exposes [profile loading](../../apps/game/README.md#movement-input-profiles).
Mouse support in InputState does not make mouse buttons valid movement-profile keys. General action mapping,
contexts, gamepad support and possession remain proposed in ADR-0010.

## Scene and asset modules

| Module                          | Current role                                                                            |
|---------------------------------|-----------------------------------------------------------------------------------------|
| scene_data / scene_io           | Schema3 parse/migration/validation, hierarchy/world matrices, bounded CPU/GPU load/save |
| editor/prefab                   | Isolated version-1 prefabs, subtree export, remapped instance keys/parents              |
| assets/scene_import             | Selected static glTF scene traversal and node/mesh metadata                             |
| assets/asset_manager, model     | CPU metadata plus cached selected-mesh GPU resources                                    |
| camera / editor camera commands | View state and validated frame/orbit/pan/dolly intent                                   |
| simulation / collision          | Default 60 Hz bounded steps; conservative overlap/swept movement foundation             |
| bounds / picking / visibility   | World-aware bounds, nearest-box picking, conservative frustum selection                 |

Schema3 can store transform-only nodes, parents, mesh selection, material tint, collider data and up to 16 scene lights.
Full static scene import is not skinning, morph playback or animation. Collision is not full physics. Model resources
still contain Vulkan types, so the asset/GPU-scene side is not backend-independent.

## Legacy and compatibility APIs

Transform and CPU editor/scene types are re-exported from engine-core. Scene/EntityId, ModelInstance, AssetManager and
SceneEditor adapt those documents to Vulkan resources. Native SceneEditor delegates authored edits/history to the core
session; legacy CommandHistory/RobotGame helpers remain compatibility leftovers rather than the normal GameApp pipeline.

`runtime::Engine` now refers to the CPU coordinator. Graphics availability uses `GraphicsProbe::probe_status()`; it
queries physical-device information without retaining a logical device. Do not confuse that status with the embedded
viewport's GPU ownership.

Procedural MeshData constructors (triangle/cube/plane/polygon/sphere/capsule) remain available with generated normals.
See [graphics and assets](../../graphics-and-assets.md). No primitive-specific persistence UI or collision shape is
implied by constructor availability.

## Learning and checks

[Steps 21–35](../../editor-steps-21-35.md) explains the exact source map, examples and
resources; [editor commands](../../scene-editing.md) describes the frontend boundary. Run runtime tests offline/locked
and use [testing](../../testing.md) for final evidence. Interactive Angular layout/child-pane acceptance, full
material/physics/import feature support and measured performance remain explicit limits.
