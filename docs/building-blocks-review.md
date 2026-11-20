# Game-building workflow review

Reviewed on 2026-11-20 against commit `1c24b9f` and the current working tree. The five blocks below describe the
earlier playable proof of concept. The original requested plan contains sixteen features across three milestones.
The [milestone ledger and walkthrough](milestones-review.md) records that complete scope and the new trigger,
interaction-settings, movement, camera and asset work. Final verification is recorded in
`docs/verification/milestones-tests-2026-11-20.json`. The inspection checkpoint in `docs-site/docs-state.json`
records the reviewed source fingerprints and verification limits.

## Status of the five blocks

| Building block | Implemented behavior | Practical boundary |
| --- | --- | --- |
| 1. Input persistence and focus reset | Project input profiles, key-code preview, configurable movement/actions, shared native-key interpretation and held-input reset at focus/mode/run boundaries | Keyboard routing is implemented; a general possession/controller system and current-turn desktop key acceptance are separate work |
| 2. Project-selected gameplay module | Undoable module selection, host-supplied module/role metadata, shared resolver in editor/player/headless/export preparation and generic behavior fallback | Modules are native compiled code; this is not arbitrary script loading or hot reload |
| 3. Authorable course bindings | Module-aware Gameplay Roles assigns or clears persistent entity keys in each scene, with history and scene persistence | Assignment can be incomplete while editing; Play and export preparation enforce the module's role requirements |
| 4. Complete and restart the run | Bound key pickup, locked exit, prepared transition to an authored Finish destination, completion, frozen completed movement, status and fresh-run restart | Progress is in memory. Stop restores authored data |
| 5. Runtime outcomes, headless replay and export parity | Shared fixed-step outcomes and scene requests, CPU command/tick/restart replay with generic Play status, stage-only snapshots and compiler preflight/staged-output parity tests | Hidden native export execution is separately evidenced below; visible desktop/another-machine acceptance remains open, and diagnostic change events are bounded |

These form a playable proof of concept. They do not establish a general-purpose physics, scripting or animation editor.
Use [project lifecycle](project-lifecycle.md) for document/run ownership and [the bundled course](apps/game/README.md#demo-progress-and-new-game)
for the reference game.

## Input and project authoring

The input manager edits the authored project's input profile through the normal typed project command path.
Its draft is scoped to the reviewed workspace generation and revision; a project switch or concurrent project edit
must not let an old whole-project Apply overwrite newer settings. Persisting those settings requires Save All or
Save Project. Native input resolution and the standalone Player use the same core key/action model.

Focused Game input is distinct from Scene navigation. Blur, focus handoff, pause, mode changes, scene activation
and restart release or suppress held input according to their existing reset boundary. The configured mapping
does not rewrite the editor's navigation shortcuts. See [input and timing](timing-input-camera.md).

The module selector is driven by `EditorCapabilitiesDto.gameplay_modules`. A descriptor provides its ID, display text,
default roles and per-scene role overrides. An empty scene override means that scene has no advertised roles;
it is different from an absent override. Older hosts that omit the catalog do not acquire invented module options.
An unavailable saved module can be inspected and repaired; selecting a different unsupported ID is rejected.
Changing the selected module preserves scene bindings and configured behaviors.

`courtyard_run` advertises Player/Key/Exit for course scenes and Player/Finish for Finish scenes.
Authored interaction settings can identify a Finish scene with a custom ID. The role controls use the configured
scene kind to expose the applicable roles while retaining the native descriptor's labels and requirements.
New Project retains the generic configured movement/spin default. Changing a module selection does not add an
unconfigured Player behavior automatically.

## Collider Inspector and sensor/filter authoring

The selected entity's Inspector now supports None, Box and Sphere, local offset, box half extents and sphere radius.
Valid previews use `set_collider` inside a grouped transaction; Enter/blur accepts the gesture and Escape or an
invalid final draft restores its baseline. Add/remove/type changes are atomic history actions. Authoring is
read-only in Play. Selection, scene and workspace changes discard stale field drafts.

The extracted collider value helpers validate finite f32 values, positive dimensions and unsigned filter integers.
They compare at engine precision so an equivalent rounded value does not add history. The edit owner coalesces
numeric previews and queues discrete interaction intents against the latest accepted dimensions and other bits.
It also flushes accepted intents before a Save or Play barrier. Correcting an invalid draft requeues a discarded
preview rather than accidentally accepting an earlier in-flight value.

Each collider can carry this optional authored interaction object:

```json
"interaction": { "sensor": true, "layers": 1, "mask": 4294967295 }
```

Absent settings default to a blocking collider on layer bit 0 with every mask bit enabled. Partial objects receive
the same field defaults. Fully default interaction settings are omitted from saved JSON, so legacy colliders
round-trip without an added object. Unknown nested fields, array/null interaction values, non-boolean sensors and
negative, fractional or overflowing u32 filters are rejected. Zero membership/mask and the highest unsigned bit
are valid. The UI names bits 0–31 as layers 1–32.

A pair is detected only when both tests pass: `(a.layers & b.mask) != 0` and `(b.layers & a.mask) != 0`.
Sensors participate in overlap queries. A sensor mover or obstacle allows movement through the overlap.
`Contact.sensor` identifies a detected pair containing a sensor. Directed Enter and Exit events are now delivered
to gameplay at fixed-step boundaries. The overlap view supplies the current accepted pairs between edges.
Tracker history and events commit with the complete simulation tick and roll back on rejected actions.

The optional **Colliders** Scene overlay displays selected effective collision geometry. Blocking outlines are
continuous cyan and sensor outlines are dashed pink, so color is not the only distinction. Game and the exported
Player omit the overlay. It is bounded to 64 selected colliders, 2,048 lines and a bounded hierarchy walk; omitted
or invalid outlines produce a separate nonfatal diagnostic. Selection/render-revision caching avoids rebuilding
unchanged outlines every frame.

Sizes and offset are local. Box collision uses the transformed world axis-aligned enclosure; a rotated box is not
an oriented-box solver. Spheres use maximum scale for orthogonal transforms and a conservative enclosing scale
under hierarchy shear. Finite local values can still produce invalid world geometry under extreme scale; debug
failure must not disable the rest of the viewport. See [scene editing](scene-editing.md#colliders-and-interaction-settings).

## Runtime completion, restart and parity

The bundled Courtyard module resolves Player, Key, Exit and Finish through scene bindings. Authored interaction
settings select the sensor entities, destination and optional exit blocker. The key is acquired once, hides and
stops colliding, and the exit changes tint and unlocks. The prepared destination starts with cleared movement.
Entering its Finish sensor completes the run. Completed progress suppresses further movement and objective work.
Projects with no interaction settings retain their earlier proximity behavior.

**Interaction Settings…** edits the current scene's override through a revision-checked project command.
**Game Camera…** edits fixed or follow settings through the scene camera command. Both finish pending Inspector
edits before building their payloads and reject stale scene or workspace contexts. Follow uses a world-space rig
with exponential position smoothing. Eye and look offsets translate together, keeping a stable viewing direction
during reversals. See the [authoring walkthrough](milestones-review.md#author-and-play-the-course).

Ordinary runtime scene changes preserve run progress and leave the authored active scene, dirty flags, history
and startup setting intact. Candidate CPU/GPU preparation precedes commit. A rejected candidate leaves the source
playable with its pending destination; retry is explicit rather than a per-frame loading storm. Restart prepares
the retained startup draft and fresh progress; it validates the source run/revision before installation. Stop
returns to authored data. Old prepared candidates cannot be applied to a replacement run.

The compiler validates every catalog scene through the same module factory before publishing staged output.
Tests exercise the route and fresh-run policy against real staged manifests/assets through the shared resolver.
This checks exported data and runtime behavior; a fresh native export remains a distinct execution check.
See [compiler](compiler/README.md) and [game status/restart](apps/editor/README.md#game-status-and-restart-run).

## Headless replay and stage-only snapshots

The bounded version-1 replay accepts `command`, `tick`, `asset_metadata` and `restart_run` actions. For example:

```json
{
  "schema_version": 1,
  "actions": [
    { "type": "command", "command": { "type": "start_play" } },
    { "type": "command", "command": { "type": "set_play_input", "movement": [1, 0, 0] } },
    { "type": "tick", "count": 60 },
    { "type": "restart_run" },
    { "type": "tick", "count": 5 }
  ]
}
```

Restart requires Play, returns to the retained startup document with fresh progress and clears held movement.
The summary's generic `play_status` reports the current module's objective/detail/score/completion and run/pending
identity; it is omitted in Edit. This avoids Courtyard-only summary fields. Compare `state_hash` for project/rendered
scene content, not whole summaries: timing and UUID run identities can differ between otherwise equivalent replays.
The replay remains bounded to 8 MiB, 10,000 actions, 100,000 tick calls and its between-action wall-time budget.

Create and exercise a frozen authored-input snapshot without compiling:

```powershell
cargo run -p compiler --offline --locked -- --stage 'D:\Authoring\Course\project.json' 'D:\Snapshots'
cargo run -p engine-headless --offline --locked -- --project 'D:\Snapshots\Courtyard Run Demo\project.json' --replay 'D:\Checks\course.json' --state-output 'D:\Checks\new-final-state.json'
```

The example assumes the authored name is Courtyard Run Demo and the snapshot parent already exists. Stage-only output
contains validated manifest/assets without a player executable. A release export remains a separate build step.
Neither staging nor headless outputs overwrite occupied destinations. Actual full-route replay tests use the bundled
layout to reach the locked exit, acquire the key, transition, complete, restart and restore authored state on Stop.

## Source map and refactor rationale

| Source | Responsibility and reason for the boundary |
| --- | --- |
| `apps/editor/src/app/modules/editor/workspace/game-input/` and `core/game-input/` | Input drafts and key preview stay in Angular; shared native profiles remain authored project data |
| `crates/engine-core/src/command.rs`, `crates/courtyard-gameplay/src/lib.rs`, host `commands/editor.rs` | Typed module metadata supplies application-owned role labels and definitions; Courtyard's settings UI selects the declared Course or Finish role set |
| `apps/editor/src/app/core/editor/gameplay-modules.ts`, workspace `scene-actions/` | Validate and present available modules/scene overrides; preserve explicit unknown or unavailable states |
| `crates/engine-core/src/collider.rs` | Collider geometry and interaction serde/default rules live together; `scene_data` re-exports preserve callers' public path |
| Workspace `collider/`, `scene-editing.service.ts` | Shape/value controls are separated from command ownership, coalescing, context guards and transaction lifetime |
| Host `collider_visuals.rs`, `viewport_interaction.rs` | Editor-only selected outlines reuse core effective geometry without running a dense overlap query on every frame |
| `crates/engine-core/src/collision.rs` | Cached and stateless collision paths share symmetric masks and sensor policy; interaction-only changes refresh cached records |
| `simulation_session.rs`, `game_session.rs`, `editor/mod.rs`, `engine.rs` under engine-core | Fixed-step rollback, typed progress, pending scene activation and diagnostic journal stay independent of GPU presentation |
| `apps/headless/src/main.rs`, `compiler/src/`, `apps/game/src/main.rs` | CPU replay, frozen export inputs and native Player installation consume the same project/module contract |

## Optimization review

These changes reduce repeated work or harden lifetime boundaries. No frame-time, loading-speed or GPU-utilization
improvement is claimed without a named before/after measurement.

- Host bounds jobs reuse the captured reference queue, cancel obsolete scene-only work and keep cancellation
  reservations until their completions are drained. Background bounds work has a 128 MiB reservation allowance,
  leaving room for an explicit foreground import. Refill does not repeatedly rescan the authored scene.
- glTF resource snapshots are cached by contained canonical source path and selected mesh. Images are prepared only
  for the selected mesh; borrowed encoded/buffer-view images avoid redundant retained copies. Decoding is bounded,
  and failed preparation leaves the previous cached entry and its accounting unchanged. The source cache is limited
  to four entries/256 MiB; pruning reconciles source and metadata retention with live model owners.
- Renderer frame state distinguishes acquisition and graphics-submission completion. Presentation semaphores are
  tied to swapchain images; failures retire invalid frame targets before reuse. Extent changes can transfer warmed
  instance storage after completion while creating fresh synchronization resources.
- Instance storage still rebuilds ordering/batches and writes its submitted instance data each draw. The renderer
  still waits on one in-flight frame; two frames in flight and persistent changed-range uploads remain future work.

See [graphics/assets](graphics-and-assets.md), [renderer ownership](crates/renderer/README.md) and the
[updated optimization TODO](../designs/found-bugs/TODO.md).

## Validation evidence and limits

The new Angular authoring surfaces passed 425 tests across 46 files and the production bundle check.
Those results are in `target/milestones-angular-tests.log` and `target/milestones-angular-production.log`.
The current Rust workspace all-target/all-feature suite passed 793 tests across 70 test binaries with 7 ignored
and no failures; see `target/milestones-rust-tests.log`. The existing project-setup stylesheet budget warning
remains. Current native, asset and compiler checks are
recorded in the [sixteen-feature ledger](milestones-review.md). The current documentation build link-checks
60 pages and passes all 7 site tests; Game App Developer remains empty. The following table preserves the previous
completion audit's evidence.

| Evidence | Observed result | Scope |
| --- | --- | --- |
| `target/completion-rust-tests.log` | 734 passed, 5 ignored | Workspace all-target/all-feature suite before the final leading-hyphen CLI regression; the three targeted final CLI checks are separate below |
| `target/completion-rust-doc-tests.log` | Passed | Rust documentation checks across ten crates; no doctest examples |
| `target/feature13-host-bridge-tests.log` | Earlier focused run: 152 passed, 1 ignored | App/bridge coverage for scene persistence/history and sensor outline style; included in the later broader validation scope |
| `target/completion-input-angular-tests.log` | 401 passed across 43 specs | Full editor suite including stale input Apply/feedback guards and collider/filter authoring |
| `target/completion-input-angular-production.log` | Production bundle passed | Existing project-setup stylesheet budget warning remains |
| `target/completion-input-host.log` | 1 passed | Real-file remapped input Save All/reopen/Play/reset/Stop regression |
| `target/completion-gameplay-tests.log` | 56 passed | Focused gameplay/headless/compiler coverage including generic status, restart replay and stage CLI |
| `target/completion-gameplay-compiler-final.log` | 3 passed | Stage CLI argument/no-overwrite checks and compatibility for a valid leading-hyphen project name |
| `target/completion-admin-tests.log`, `target/completion-python-tests.log` | 15 admin tests and 51 Python tests passed | Current completion audit; separate from frontend/Rust totals |
| `target/completion-native-transition.log` | 1 passed | Hidden window with real Vulkan: restart/transition resource reuse and failed partial-load run preservation |
| `target/completion-renderer-smoke.log` | Passed | Hidden Vulkan rendering/resource lifetime, resize and zero-size behavior; smoke counters are not a benchmark |
| `target/completion-native-probe.log` | 1 passed | Native Vulkan initialization/status probe |
| `target/completion-release-export.log` | Release package produced | Fresh compiler output outside the checkout; exported executable matches the built release (3,849,216 bytes) |
| `target/completion-gameplay-export.log` | Stage-only snapshot produced | Actual new manifest/assets snapshot outside the checkout, with no compilation/executable |
| `target/completion-gameplay-replay.log` | Six replay phases passed | CPU runner against the completed release export: locked exit, key, finish, completion, restart and Stop; nine source assets remain byte-identical |
| `target/completion-export-native.log` | 1 passed | Hidden GameApp/Vulkan harness uses the fresh exported manifest/assets: locked exit, key, transition, completion freeze, F10 restart and neutral input; direct ApplicationHandler focus-event dispatch checks reset |
| `target/completion-launch-contract.log` | 1 passed | Explicit release-build opt-in verifies executable path, working directory, Player argument and tamper rejection; it does not launch an interactive game window |
| `target/completion-native-editor-build.log` | Passed | Current native editor build with custom protocol; build success is not a visible editor smoke |
| `target/completion-format-check.log`, `target/completion-clippy.log` | Passed | Final formatting and Clippy checks; existing argument-count/enum-size/dead-code/manual-clamp warnings remain |
| `target/completion-docs-site.log` | 58 pages, 7 tests passed | Documentation build/link/space checks; Game App Developer remains empty |
| Current-turn visible desktop course | Not performed | The exported native test drives actual player code through a hidden harness; it does not interactively drive the standalone exported executable |

The 2026-11-15 checkpoint records earlier release-export validation, a generic editor status/restart/Stop smoke,
and a copied exported Player fixture that completed and restarted the course. Those are historical observations
with fixture-specific limits, not fresh acceptance of the current sensor/filter and renderer changes.
The documentation-site build checks publication, navigation and links, not game correctness or native appearance.
The hidden focus regression dispatches the production event handler directly. OS-generated focus/keyboard delivery,
cross-window capture and visible input interaction remain separate checks.
Four of the five default-skipped native/release tests were run explicitly and passed. The unrelated admin
copied-template native validation was not run in this completion audit. Overlapping focused/native totals are not
added to the workspace count as though they were one unique suite.

## Remaining work

The [sixteen-feature ledger](milestones-review.md) records feature coverage and the aggregate verification status.
Collision uses conservative world bounds and bounded sweep/slide. Native compiled modules own gameplay rules.
Material, light and general behavior Inspector sections, linked prefab overrides, an animation editor,
runtime apply-back and persisted progress remain separate product work. Full desktop input acceptance and
another-machine export acceptance require their own recorded results.
