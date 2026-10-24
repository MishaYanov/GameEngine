# Standalone game runtime

Source: `apps/game`. **Normal execution is a game/player path, not an editor overlay UI.** The editor product belongs to
the Angular app and its chosen embedded game pane. Runtime exposes the reusable logic and the Tauri host owns native
rendering integration.

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

RobotGame is demo-specific behavior. EditorSession is a separate CPU document application service for Angular commands;
the game executable is not a replacement Angular UI or a location to hardcode all editor shortcuts.
See [steps 21–35](../../editor-steps-21-35.md), [runtime](../../crates/runtime/README.md)
and [renderer](../../crates/renderer/README.md).

## Explicit legacy editor demo

To inspect the earlier native prototype deliberately:

```powershell
$env:GAME_ENGINE_NATIVE_EDITOR_DEMO = '1'
cargo run -p game --locked --offline
Remove-Item Env:GAME_ENGINE_NATIVE_EDITOR_DEMO
```

This opt-in path retains prior native panels, selection/gizmos and keyboard demonstration controls. They are not
composed into Angular and do not define bindings for the typed runtime editor. See the
historical [steps 11–20 report](../../editor-steps-11-20.md). Production default stays overlay-free.

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
