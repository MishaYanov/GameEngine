# PROJECT3 handoff: authored projects and backend steps 46–55

This handoff describes the steps 36–45 backend working tree inspected on 2026-10-24 and supplies a prompt for the next
chat. HEAD remains `a48ff6c`; substantial implementation and tests are uncommitted. The commit alone does not identify
this state. Current verification is recorded below; the earlier [steps 21–35](editor-steps-21-35.md) counts do not
verify this batch. Existing project guides may still describe the earlier architecture: this focused update does not
broadly reconcile them.

## Current authoring-to-player workflow

```text
Editor-authored ProjectData v1 + SceneData v3 + assets
        shared CPU Engine / EditorSession
        authored scene and project settings / undo / explicit saves
        compiler prepared project + captured asset fingerprints
        built player package
        configured SimulationSession (also used by editor Play)
```

`ProjectData` chooses the startup scene, fixed-step settings and explicit persistent-key behavior targets.
`controlled_move` and `spin` are the current configured behaviors. A default scene project has no implicit game
behavior. The bundled demo opts into behavior through `apps/game/project.json`; demo role names are not reusable engine
policy.

`engine-core` owns CPU data, authoring commands, world state and simulation. Its dependencies exclude renderer, bridge,
logger and winit. The runtime crate supplies GPU adapters, while hosts handle files, windows and IPC. `EngineBuilder`
constructs the typed coordinator; commands produce receipts, stable error categories and bounded change pages. Extend
this pipeline rather than introducing another competing editor command system.

Scene and project edits share bounded undo/transactions but have independent saved baselines. Host `save` saves the
scene baseline; `save_project` saves project configuration. Reload validates both candidates. Native development F5
saves authored scene and project as separate atomic-file operations, marking the combined state clean after both
succeed. This is not a multi-file atomic transaction: one write may succeed before another fails.

Host `build_project` requires clean Edit state and matching disk content, prepares the project, and releases the
workspace lock before compilation. Compiler preflight checks presence, format and referenced-model snapshot membership;
detailed glTF validation remains in the runtime importer. A prepared fingerprint detects changed inputs within its
contract, not malicious tampering or universal reproducible-build guarantees.

Source `cargo run -p game` uses Development mode, preserving the existing panels, gizmos and learning controls. An
installed project marker selects Player mode; `--development` and `--player` select it explicitly. This supersedes the
prior batch's source-default overlay-free description. The native tools are a transitional harness. The future product
editor remains Angular/Tauri and owns the game pane/window; this batch adds no Angular UI or frontend wiring.

## Implemented steps 36–45

Examples below identify current APIs or command intent; omitted arguments depend on the caller's validated document.
Each row ends with further reading.

| Step | Responsibility, example and reason                                                                                                                                                                                                                | Primary reading                                                                                                                         |
|------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------|
| 36   | Launch policy: `LaunchOptions::parse(["--player"], installed_project, legacy_demo)` makes mode explicit; source defaults to Development and installed projects to Player, preserving the learning harness without making it player UI.            | [Rust environment and arguments](https://doc.rust-lang.org/std/env/)                                                                    |
| 37   | CPU engine extraction: `EngineBuilder::new(scene, project).build()` constructs validated state without a GPU/window dependency, so host, player and headless tools share engine semantics.                                                        | [Rust builders](https://rust-lang.github.io/api-guidelines/type-safety.html#builders-enable-construction-of-complex-values-c-builder)   |
| 38   | Typed command construction: `CommandFactory::builder(revision).command(command).build()` creates a validated serializable request; explicit command variants and stable errors keep callers independent of keys/DOM.                              | [Rust enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html)                                                              |
| 39   | Native delegation: `SceneEditor::for_project(...)` uses core `EditorSession` history and transactions, so native edits and host commands follow the same authored-state rules instead of maintaining competing undo stacks.                       | [Command pattern](https://gameprogrammingpatterns.com/command.html)                                                                     |
| 40   | Configured gameplay: `ProjectData` names `controlled_move`/`spin` targets by persistent key; `SimulationSession` runs the same module behavior in Play and Player, separating reusable policy from the robot demo.                                | [Fixed timesteps](https://gafferongames.com/post/fix_your_timestep/), [Rust traits](https://doc.rust-lang.org/book/ch10-02-traits.html) |
| 41   | Incremental world/resources: `RuntimeWorld::set_transform(...)` updates affected subtrees, while `AssetRef` identifies cached runtime resources. Stable ownership and dirty changes reduce repeated full reconstruction.                          | [Dirty flags](https://gameprogrammingpatterns.com/dirty-flag.html), [Rust Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html)      |
| 42   | Observable state: `Engine::execute(...)` returns a receipt and `changes_since(cursor)` returns bounded typed changes or resync, so consumers need not treat every frame as a full document response.                                              | [Event queues](https://gameprogrammingpatterns.com/event-queue.html)                                                                    |
| 43   | Persistent collision: `CollisionWorld::move_entity_with_budget(...)` reuses collider geometry/order and charges bounded work; conservative blocking and tick rollback protect state under invalid or excessive movement.                          | [Rapier collider concepts](https://rapier.rs/docs/user_guides/rust/colliders/) — conceptual reading, not an implementation dependency   |
| 44   | Bounded CPU jobs: `try_submit`, `cancel` and drain operations use generation/revision guards; `Engine::replace_entities_with_assets` atomically adopts valid results. Importing stays off the render owner and workers never upload GPU data.     | [Rust bounded channels](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html)                                                   |
| 45   | Headless acceptance: `engine-headless --project apps/game/project.json --steps 600` runs shared simulation/replay with resource counters and bounded inputs, making CPU behavior inspectable without equating timings with GPU/visual acceptance. | [Rust Instant](https://doc.rust-lang.org/std/time/struct.Instant.html)                                                                  |

Legacy runtime `CommandHistory` and `RobotGame` APIs remain compatibility leftovers. Normal `GameApp` uses the core
path; PROJECT3 should not extend those leftovers into a new parallel engine pipeline.

## Source and API map

Paths below are relative to `D:\Coding\Game Engine`.

| File or module                                                                         | Responsibility and useful entry points                                                                                         |
|----------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/engine.rs`                                                     | `EngineBuilder`, CPU `Engine`, `CommandReceipt`, `BackendErrorCode`, execute/tick/change pages and independent saved baselines |
| `crates/engine-core/src/command.rs`, `editor/`                                         | Serializable command DTOs, capability/dispatch factory, `EditorSession`, project settings, history, transactions and prefabs   |
| `crates/engine-core/src/project.rs`                                                    | `ProjectData`, `BehaviorSpec`, version/target validation and authored configuration                                            |
| `crates/engine-core/src/simulation_session.rs`                                         | Shared `SimulationSession`, `GameModuleFactory`, configured behavior modules, bounded action planning and rollback             |
| `crates/engine-core/src/world.rs`, `collision.rs`                                      | `RuntimeWorld` subtree matrix cache, persistent collision geometry/order and changes                                           |
| `crates/engine-core/src/events.rs`                                                     | Typed capped change journal and resynchronization contract                                                                     |
| `crates/engine-core/src/asset_registry.rs`, `asset_jobs.rs`                            | Scoped generational asset handles, CPU metadata, bounded importer worker/cancellation/draining                                 |
| `crates/runtime/src/assets/metadata_import.rs`, `import_budget.rs`, `asset_manager.rs` | Real bounded metadata import and renderer-owned asset adapters; `AssetRef` / `load_asset` integration                          |
| `crates/runtime/src/scene.rs`, `scene_editor.rs`                                       | GPU scene synchronization and native compatibility harness                                                                     |
| `apps/editor-host/src-tauri/src/editor.rs`, `commands/editor.rs`, `viewport.rs`        | Project workspace, IPC/filesystem/build/import operations and native render owner                                              |
| `apps/game/src/launch.rs`, `game.rs`, `main.rs`, `project.json`                        | Development/Player modes, configured player simulation and explicit demo configuration                                         |
| `compiler/src/lib.rs`, `main.rs`                                                       | Prepare/install authored projects and package validated captured inputs                                                        |
| `apps/headless/src/main.rs`                                                            | `engine-headless` CPU project/scene runner and bounded replay                                                                  |

Host endpoints include `editor_open_project`, `editor_execute_receipt`, `editor_changes`, `editor_save_project`,
`editor_build_project`, and import submit/cancel/drain, alongside existing open/save/reload. Their presence does not
mean Angular has adopted those endpoints yet.

## Budgets and acceptance limits

- Simulation ticks aggregate at most 4,096 planned actions and a one-million collision-work budget. The matrix-work
  threshold is one million updates; a single action can overshoot by up to scene size before detection. Failure rolls
  back and pauses rather than committing partial simulation.
- The change journal retains at most 256 entries / 1 MiB; change keys cap at 4,096 and oversized/expired changes require
  resynchronization.
- CPU asset jobs use one worker, eight pending jobs and 256 MiB reservations, with notifier/drain/cancel and stale
  generation/revision guards. Workers do not upload GPU resources. Logical byte budgets are not total process or driver
  memory measurements.
- Registry batches are staged atomically. `Engine::replace_entities_with_assets` commits scene entities, metadata and
  history together or preserves the previous state. Bounds entries represent selected meshes, including nondefault mesh
  indices; reload clears registry/bounds state. Collision membership changes invalidate the persistent cache, and
  process-wide unique job IDs prevent cross-broker cancellation aliases.
- Worker errors are capped at 2,048 UTF-8 bytes and compacted before queueing. Host completions cap message text at
  1,024 bytes, retain at most 64 entries / 128 KiB using conservative escaped-size accounting, and expose stable
  `error_code` values. These caps bound retained diagnostics, not arbitrary external allocations.
- GPU scene adapters apply changes incrementally. Structural changes and resource retirement still require correct
  render-owner synchronization; this is not a claim that every operation is allocation-free or nonblocking.
- FIFO presentation and demand pacing remain, with continuous cadence capped around 60 scheduling intervals per second.
  Hardware cadence and speedup are unmeasured.
- Static glTF, conservative collision, basic lighting, and existing prefab copy semantics retain their earlier feature
  limits. No full physics, animation/skinning, live prefab inheritance or general scripting platform is implied.

Run the CPU demo from the repository root:

```powershell
cargo run -p engine-headless --offline --locked -- --project apps/game/project.json --steps 600
```

The headless runner loads no GPU, window or model geometry. Replay is capped at 8 MiB, 10,000 actions and 100,000 tick
calls; time budgets are checked between actions, not as hard interruption deadlines. Optional supplied asset metadata
supports CPU bounds operations. A static project may perform zero simulation steps by design. FNV-1a 64-bit
project/render-scene hashes are diagnostic, not cryptographic; UUID-generating commands are explicitly nondeterministic.
Debug CPU timings are illustrative, not performance acceptance.

## Verification

The following engine results come from final implementation/TESTER/reviewer handoffs for this batch, with saved logs
under `target/steps36-45`. Documentation maintenance inspected the logs and ran the documentation checks separately; it
did not rerun engine tests or modify production/test files.

| Check                                     | Result and scope                                                                                                                                                                        |
|-------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Workspace all-target offline/locked tests | **382 passed, zero failed, two ignored**; TESTER alone added 88 regression tests. Log: `qa-workspace-final.log`                                                                         |
| Supplemental ignored checks               | GraphicsProbe/native initialization and copied native-client builder validation each passed explicitly; the standard ignored count remains two. Copy log: `qa-copied-native-client.log` |
| Hidden GPU examples                       | Current-source `renderer_smoke` and `scene_io_smoke` passed; execution assertions do not establish visible appearance or interaction                                                    |
| Compile and Clippy                        | Workspace bins/lib/examples check passed; workspace all-target Clippy passed with inherited warnings; core/headless Clippy passed with warnings denied                                  |
| Test formatting and whitespace            | TESTER reported changed-test formatting and diff checks passing                                                                                                                         |
| SECURITY                                  | Final static re-review closed both import findings and found no remaining actionable defects in the reviewed scope; no test or GPU acceptance implied                                   |
| LOGGER                                    | Targeted stable operation categories, gated DEBUG launch details and failure latches passed production checks; no per-frame logging flood or full logging-infrastructure audit claimed  |
| Documentation                             | 28 pages rebuilt/link-checked; all five site tests passed; 430 local/cross-page anchors resolved; focused diff checks passed                                                            |

CPU resource fixtures ran 120 fixed steps with one collision-cache rebuild each. The 1,000-entity fixture recorded 1,240
matrix updates, 1.9785 ms total tick time, 0.1953 ms maximum tick time and 30.104 ms total run time. The 10,000-entity
fixture recorded 10,240 updates, 3.2932 ms total tick time, 1.5488 ms maximum tick time and 278.9898 ms total run time.
Each performed 240 matrix updates beyond initialization. These are debug observations from
`qa-headless-resource-final.log`, not timing thresholds or speedup claims. The configured source project separately
produced 600 fixed steps, 1,202 matrix updates and one collision rebuild in 600 tick calls.

`docs-site/docs-state.json` records the inspection baseline, uncommitted tree, evidence and limits. No visible native
interaction, Angular pane composition, installed release execution, clean-checkout reproducibility or Jenkins acceptance
is claimed. Copied-client compilation/validation does not certify installed player execution.

## Copy-ready PROJECT3 prompt

Copy the following block into PROJECT3. It proposes future work; it does not authorize implementation of the next batch.

```text
We are continuing GameEngine2 at D:\Coding\Game Engine in PROJECT3.
Read AGENTS.md and docs/project3-handoff.md first, including its final verification
section and known limits. Inspect current source, Git history, staged/unstaged/
untracked changes and docs-site/docs-state.json. HEAD/checkpoint are inspection
baselines, not proof that the dirty tree matches existing docs. Preserve all work;
do not stage, commit, reset or discard changes unless I explicitly request it.

Teaching-first default: explain responsibilities, interface, exact files, reasoning
and a focused example, then let me implement. Only edit production code when I
explicitly ask you to implement or fix it. End every step with primary reading
resources. A dedicated TESTER writes tests; do not assign test writing to me or
write tests in the coding agent. Inspect TESTER's actual final results before
claiming verification. Ask for scoped LOGGER/SECURITY review when authorized.
Only update docs on an explicit request, then rebuild/check the static site and
record baseline, working tree and verification limits. Engine Developer owns the
engine docs; leave Game App Developer empty unless requested.

The product is an Angular editor hosted by Tauri, containing controls and a game
pane. CPU engine logic belongs in engine-core; runtime owns GPU adapters and the
host owns windows/files/IPC. Preserve the native Development harness, all its
existing controls, and the installed Player mode. Do not build a new native UI,
compose Angular screens, wire new frontend features or create another chat unless
asked. Typed intent must work independently of UI keys. Continue the proven
command/receipt/history/change-journal path rather than adding a parallel one.

Current foundation: authored ProjectData v1 + SceneData v3 + assets feed compiler
preparation and the player; editor Play and Player share SimulationSession.
Scene/project settings share undo but keep distinct saved baselines. EngineBuilder,
typed coordinator/errors/receipts, cached RuntimeWorld, persistent CollisionWorld,
generational asset registry, bounded CPU import jobs, incremental GPU adapters and
headless replay exist. Reinspect before extending them. No visible native UI
acceptance has been performed; user selected automated-only checks. Logical byte
budgets do not measure total process/driver memory. No measured speedup claim.

Proposed backend steps 46-55 (adjust only after checking existing implementation):

46. Project lifecycle, root and manifest ownership.
    Extend existing open/save/reload/build contracts into one explicit project
    lifecycle. Explain canonical root/path ownership and failure preservation.
    Example: open project A, reject an invalid replacement, retain A unchanged.
    Reading: https://rust-lang.github.io/api-guidelines/type-safety.html

47. Multi-scene authoring.
    Add stable scene identities, per-scene dirty/save state and safe switching;
    preserve the current single-startup-scene flow and project baseline rules.
    Example: switch away from a dirty scene without silently losing edits.
    Reading: https://gameprogrammingpatterns.com/dirty-flag.html

48. Typed component/behavior schema and factory registry.
    Generalize the existing ControlledMove/Spin descriptors and module factory
    with versioned validation, explicit registration and bounded configuration.
    Example: reject unknown behavior kinds before mutating an authored project.
    Reading: https://doc.rust-lang.org/book/ch10-02-traits.html

49. Command registry, capabilities and serialization readiness.
    Extend the existing typed commands/factory/capabilities/stable errors and
    undo pipeline; avoid another dispatcher or duplicate protocol source.
    Example: advertise a command and reject a stale revision consistently.
    Reading: https://gameprogrammingpatterns.com/command.html

50. Authored player action maps.
    Map device input to game intent independently of editor shortcuts, retaining
    focus-loss clearing, fixed-step behavior and bounded input handling.
    Example: bind two devices to one movement action without changing simulation.
    Reading: https://gafferongames.com/post/fix_your_timestep/

51. Linked prefab instances and overrides.
    Extend current copied prefabs with source identity, instance ownership,
    explicit overrides and safe update/revert behavior; preserve key remapping.
    Example: update a source transform while keeping an instance's tint override.
    Reading: https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html

52. Asset dependencies, invalidation and bounded upload ownership.
    Extend generational assets/jobs into dependency tracking and stale-result
    invalidation; uploads and retirement stay with the render owner.
    Example: a changed model invalidates dependents without accepting old jobs.
    Reading: https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html

53. Incremental, cancellable and reproducible build sessions.
    Extend prepared fingerprinted projects with explicit build identity,
    dependency snapshots, cancellation points and artifact validity rules.
    Example: cancellation or changed inputs never publish a successful stale build.
    Reading: https://doc.rust-lang.org/std/fs/

54. Player lifecycle and scene transitions.
    Derive player initialization, pause/stop/restart and validated scene changes
    from the shared backend; keep editor-authored and simulated state separate.
    Example: a failed scene transition retains the previous playable world.
    Reading: https://gameprogrammingpatterns.com/state.html

55. Backend acceptance and bridge readiness before Angular work.
    Consolidate resource, replay, lifecycle, build and error acceptance with
    TESTER; document protocol versions, resync and limits for future UI adoption.
    Example: replay a bounded edit/play/build scenario and inspect exact results,
    without treating a diagnostic hash or hidden smoke as visual acceptance.
    Reading: https://gameprogrammingpatterns.com/event-queue.html

Start with step 46: inspect what already exists, explain only the missing
responsibilities, proposed interface, exact changes, example and resources.
Do not implement it yet. Do not silently reimplement completed foundation work.
```
