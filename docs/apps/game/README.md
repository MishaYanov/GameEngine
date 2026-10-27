# Standalone game runtime

Source: `apps/game`. The source executable defaults to Development mode with the existing learning panels, gizmos and
controls. Installed projects default to Player mode. The future product editor remains Angular/Tauri; native tools are a
transitional harness over shared core semantics.

## Run

From the root after [setup](../../usage.md):

```powershell
cargo run -p game --locked --offline
```

The native runtime needs Vulkan 1.3, a supported driver, shader compiler, and debug validation layers. Scene assets
resolve below the development game asset root or beside an installed/name-file executable. Preserve scenes, models,
images and external dependencies together.

## Runtime foundations

Steps 21–35 add hierarchy/world transforms, scene schema3, static glTF node/mesh import, per-instance tint,
directional/point lights, bounded fixed steps and conservative collision movement. Renderer submission adds asynchronous
GPU timestamps and compatible opaque instancing. Actual model/material/scene inputs remain validated and bounded.

## Modes and authored gameplay

Use `cargo run -p game -- --development` or `cargo run -p game -- --player` to choose explicitly. Installed project
detection selects Player by default; the older environment switch remains a compatibility input. Development retains
existing controls, but native SceneEditor delegates edits/history to core EditorSession.

Project schema 2 identifies startup_scene_id in its scene catalog. The player loads that document and applies its
scene-scoped ControlledMove/Spin behaviors through the same SimulationSession used by editor Play. RobotGame and older
CommandHistory APIs remain compatibility helpers, not the normal GameApp behavior pipeline.
See [project lifecycle](../../project-lifecycle.md), [backend handoff](../../project3-handoff.md)
and [runtime](../../crates/runtime/README.md).

Multi-scene authoring is not automatic gameplay scene transitions. Keep the manifest, scenes, models and asset
dependencies together in an installed project.

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

Refer to [testing](../../testing.md) and the current report for final counts/evidence. Historical Windows output-access
problems and steps 11–20 counts describe that batch; they should not be treated as current failures or new visual
acceptance.
