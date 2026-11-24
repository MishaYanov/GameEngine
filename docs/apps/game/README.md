# Standalone game runtime

Source: `apps/game`. The source executable defaults to Development mode with the native learning tools.
Installed projects default to Player mode. The Development tools and the Angular/Tauri editor use shared core
behavior and scene semantics.

## Run

From the repository root, launch the bundled Cinder Oath game in Player mode. [Setup](../../usage.md).

```powershell
cargo run --locked --offline -p game -- --player
```

The source launch opens ``projects/cinder-oath/project.json``. An installed Player opens the ``project.json``
beside the executable. Keep that manifest and its ``assets`` directory together. Development builds need
Vulkan 1.3 and the Vulkan shader compiler.

## Runtime foundations

The Player resolves object hierarchy, model and material assets, and lighting from the saved project. It advances
gameplay with bounded fixed steps. Runtime synchronizes the accepted state with the renderer and updates the
window, native input and audio. [Scene, simulation and rendering pipeline](../../pipeline.md).

### Open an authored project

Use **File → Open Project…** in the Editor to open a project manifest. The standalone Player opens the manifest
in its own package. Export a project from the Editor or use the compiler's `--export` command.
For development checks, open the Primitive Workshop or Conditional Authoring fixture under
`apps/headless/tests/fixtures`. Use the [headless runner](../../testing.md#headless-replay) to exercise
the same simulation paths from an automated replay.

## Modes and authored gameplay

Use `cargo run -p game -- --development` or `cargo run -p game -- --player` to choose explicitly. Installed project
detection selects Player by default; the older environment switch remains a compatibility input. Development retains
existing controls, but native SceneEditor delegates edits/history to core EditorSession.

Project schema 3 identifies `startup_scene_id` in its scene catalog. The player loads that document and applies its
scene-scoped ControlledMove/Spin behaviors, including shared settings, through the same SimulationSession used by
editor Play. Both paths resolve the manifest's optional `gameplay_module` through `gameplay-catalog` when Play
starts. An omitted selector uses configured behaviors. GameSession owns the current simulation and explicit
replacement boundary. RobotGame and older
CommandHistory APIs remain compatibility helpers, not the normal GameApp behavior pipeline.
See [project lifecycle](../../project-lifecycle.md), [workspace architecture](../../architecture.md)
and [runtime](../../crates/runtime/README.md).

Keep the manifest, scenes, models and asset dependencies together in an installed project.

## Player scene switching

The selected gameplay module can request a scene transition. Cinder Oath moves from the introduction into
the vault after the introduction finishes or the player skips it. The destination comes from the project catalog.

The switch resolves the catalog path canonically inside the asset root, parses the document and prepares a CPU
simulation candidate. A separate AssetManager and GPU scene/camera are then built. The old scene stays installed
until those steps and `renderer.wait_idle()` succeed. Commit rejects foreign/stale candidates, clears prior movement,
resets the native movement target/selection, and suppresses held input until release. After commit, the caller replaces
the rendered scene/assets/camera without intervening fallible operations and resets frame scheduling.

A failed preparation preserves the installed scene and reports an update error. Switching builds a fresh runtime
world for the destination and retains the run's game-owned progress. The manifest's startup scene and the authored
documents keep their existing values.

## Play and restart Cinder Oath

Start at the introduction. Press **Enter** to skip to the vault. Use **W**, **A**, **S** and **D** to move,
and press **F** to attack when close to the Warden. Three landed hits finish the encounter.
In Editor Play, focus the Game view and use **Restart Run** to return to the startup scene with fresh progress.
Use **Stop** to return to authoring.

The shared GameSession retains game-owned progress during a scene transition.
[Typed progress](../../crates/runtime/README.md#game-progress) · [Windows quickstart](../../windows-editor-quickstart.md).

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

See [Compiler packaging](../../compiler/README.md) for the files the Player uses at startup.

## Checks

```powershell
cargo check --workspace --all-targets --locked --offline
cargo test -p runtime -p game -p renderer --all-targets --locked --offline
```

Use [Testing and validation](../../testing.md) for replay and windowed acceptance.
