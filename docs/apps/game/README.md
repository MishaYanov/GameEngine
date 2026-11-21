# Standalone game runtime

Source: `apps/game`. The source executable defaults to Development mode with the native learning tools.
Installed projects default to Player mode. The Development tools and the Angular/Tauri editor use shared core
behavior and scene semantics.

## Run

From the repository root, build and launch the bundled Courtyard Run in Player mode. [Setup](../../usage.md).

```powershell
cargo run --locked --offline -p game -- --player
```

The native runtime needs Vulkan 1.3, a supported driver, shader compiler, and debug validation layers. The source
launch loads `apps/game/project.json` and `apps/game/assets`. A `project.json` beside the executable takes precedence,
and a legacy adjacent `project-name.txt` also selects installed assets if the manifest is absent. Run from a clean Cargo
target directory if a previously installed project occupies the executable directory. Keep the manifest, scenes,
models and their dependencies together in a distribution.

## Runtime foundations

Steps 21–35 add hierarchy/world transforms, scene schema3, static glTF node/mesh import, per-instance tint,
directional/point lights, bounded fixed steps and conservative collision movement. Renderer submission adds asynchronous
GPU timestamps and compatible opaque instancing. Actual model/material/scene inputs remain validated and bounded.

### Event, controller and animation samples

Additional authored manifests are `apps/game/event-demo.json`, `controller-demo.json` and `animation-demo.json`.
Open the selected manifest in the editor, use the CPU runner with `--project`, or export that manifest with the
compiler's `--export` form and run the resulting package. The source Player launch above still selects the bundled
default project; it does not accept these manifests as a `--project` argument.

The event lab demonstrates independent timed doors and one-shot pickup/completion; Controller Room uses WASD and
Space for upright box movement; Animated Character Room adds Idle/Walk/Air plus an independent looping companion.
Gameplay, event queues, movement and animation share the accepted fixed-step transaction. See the
[M7–M9 walkthrough and commands](../../milestones-7-9-review.md#run-and-verify) for asset restrictions, native
preview, source pins, replay diagnostics and acceptance boundaries.

## Modes and authored gameplay

Use `cargo run -p game -- --development` or `cargo run -p game -- --player` to choose explicitly. Installed project
detection selects Player by default; the older environment switch remains a compatibility input. Development retains
existing controls, but native SceneEditor delegates edits/history to core EditorSession.

Project schema 3 identifies `startup_scene_id` in its scene catalog. The player loads that document and applies its
scene-scoped ControlledMove/Spin behaviors, including shared settings, through the same SimulationSession used by
editor Play. Both paths resolve the manifest's optional `gameplay_module` through `courtyard-gameplay` when Play
starts. An omitted selector uses configured behaviors. GameSession owns the current simulation and explicit
replacement boundary. RobotGame and older
CommandHistory APIs remain compatibility helpers, not the normal GameApp behavior pipeline.
See [project lifecycle](../../project-lifecycle.md), [backend handoff](../../project3-handoff.md)
and [runtime](../../crates/runtime/README.md).

Keep the manifest, scenes, models and asset dependencies together in an installed project.

## Player scene switching

The authored exit in the bundled course requests the `finish` scene. During an incomplete run, **F8** also cycles
to the next catalog scene in lexicographic scene-ID order and wraps at the end. It has an effect when there are at
least two scenes and gameplay input leaves that shortcut available. With a failed transition pending, F8 retries
the requested destination. The Courtyard, Finish and Robot Demo actors share `player_move` settings and have their
own entity keys and runtime worlds.

The switch resolves the catalog path canonically inside the asset root, parses the document and prepares a CPU
simulation candidate. A separate AssetManager and GPU scene/camera are then built. The old scene stays installed
until those steps and `renderer.wait_idle()` succeed. Commit rejects foreign/stale candidates, clears prior movement,
resets the native movement target/selection, and suppresses held input until release. After commit, the caller replaces
the rendered scene/assets/camera without intervening fallible operations and resets frame scheduling.

A failed preparation preserves the installed scene and reports an update error. Switching builds a fresh runtime
world for the destination and retains the run's game-owned progress. The manifest's startup scene and the authored
documents keep their existing values.

## Demo progress and New Game

The bundled manifest selects `courtyard_run` and starts in Courtyard. The same role-based rules run in Editor Play,
headless Play, and the standalone Player. The default movement keys are W, A, S and D.

1. Move the capsule with **A** toward the green exit at X = −1.5. The title reports **Exit: locked** while the key is
   missing.
2. Move with **D** toward the yellow key sphere at X = 1.5. Entering its 0.35-unit proximity awards one point, records
   the key and unlocks the exit.
3. Return with **A** to the green exit. The game loads Finish automatically. Release the movement key before moving
   again so input starts fresh in the new scene.
4. Move with **D** toward the blue Finish cube at X = 1.5. Approaching it after taking the courtyard exit completes
   the run and freezes gameplay movement. The title displays **Complete! F10 to restart**.
5. Press **F10** to start a fresh run in Courtyard with score zero and the exit locked.

The title reports runtime scene, score, key ownership and exit state when the scene has an exit role. **F7** adds one
debug point during an incomplete campaign run. New Game prepares fresh progress and a startup simulation before
adopting CPU/GPU state. Ordinary scene travel retains the run's in-memory progress. The demo's capsule, cube and
sphere are packaged as reusable glTF models under
`apps/game/assets/props`. [Typed progress contract](../../crates/runtime/README.md#game-progress).

## Movement input profiles

The native window adapter maps winit physical keyboard keys and mouse buttons into platform-independent runtime
input types, preserving unidentified platform scan codes and extra mouse-button IDs. The default gameplay movement
mapping is WASD. To choose another four-key mapping, pass a UTF-8 JSON file:

```powershell
cargo run -p game -- --player --input-profile .\movement.json
```

```json
{
  "schema_version": 1,
  "movement": {
    "negative_x": "arrow_left",
    "positive_x": "arrow_right",
    "negative_y": "arrow_down",
    "positive_y": "arrow_up"
  }
}
```

`--input-profile` is accepted once in either launch mode; the default profile is used when omitted. The loader requires
a regular file, reads at most 4 KiB plus one overflow byte and rejects invalid UTF-8, unknown fields or unsupported
schema versions before opening platform/GPU resources. Relative paths follow the process working directory. Profiles
are explicit launch inputs; scene/project files do not automatically store or select them.

The profile changes controlled gameplay movement, including the native development harness's Play path. Camera arrows
and editor tool shortcuts retain their own controls and may overlap a chosen mapping. Held configured movement keys
can wake a static Play session; changing bindings or a movement target clears old movement and suppresses held input
until release. See [runtime input contracts](../../crates/runtime/README.md#physical-input-and-movement-profiles).

## Resource and timing limits

WindowRenderer owns GPU device/presentation resources; uploaded scene/cache owners must still release after idle and
before their device. FixedStepper defaults to 60 Hz with bounded catch-up; collision uses conservative bounds and is not
a rigid-body solver. GPU timestamps measure an older completed command range and may be unavailable; draw/instance
counts alone do not establish performance improvement.

The standalone game schedules continuous redraws with WaitUntil at update-start plus approximately 16.667 ms, including
FIFO draw time in that deadline. Input and resize can wake it immediately; paused/static, unfocused, occluded and
minimized states wait. This is scheduling policy, not measured hardware cadence or a speedup result.

Compiler and admin source generation now include bounded validated asset JSON. This fixes the prior missing robots.json
allowlist gap, but installed execution and full distribution acceptance require their own results.

## Checks

```powershell
cargo check --workspace --all-targets --locked --offline
cargo test -p runtime -p game -p renderer --all-targets --locked --offline
```

A Windows smoke placed the route markers at their respective player spawns in a copied export. The Player
displayed the Finish marker and completion in the title. After restoring the exported Courtyard document in
the copy, F10 returned to Courtyard with score zero and the exit locked. Automated checks cover the full route
and restart from authored and staged data. [Repository testing](../../testing.md).
