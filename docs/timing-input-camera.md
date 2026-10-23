# Timing, input, and camera — in progress

**Status: In progress.** This guide describes the current staged/unstaged implementation of
`crates/runtime/src/time.rs`, `input.rs`, and `camera.rs`, the new `camera_controller.rs`, plus integration in
`apps/game/src/main.rs` and robot rules in `game.rs`. The native editing extension is implemented and automated
tests/hidden examples are reported in [steps 11–20](editor-steps-11-20.md). Human-visible control acceptance remains
unverified; no complete gameplay framework is claimed.

## time.rs: FrameClock

`runtime::FrameClock` (also `runtime::time::FrameClock`) stores the previous `Instant`. `tick(now) -> Duration` sets the current baseline and returns the elapsed interval, capped at **100 ms**. The first tick returns zero. Equal/backwards timestamps return zero through saturating subtraction, and the next tick uses the newly supplied baseline. Time beyond the cap is discarded, not accumulated for later catch-up.

`reset()` removes the baseline, so the next tick returns zero. The game ticks on redraw, converts the duration with `as_secs_f32()`, and scales movement/rotation by that delta. It resets the clock on focus loss and zero-sized resize. This is a variable-step prototype, not a fixed-step simulation or real-time pacing guarantee.

## input.rs: key state

`runtime::{InputState, Key}` tracks held keys and per-frame pressed/released transitions independently of winit. Current
keys are W/A/S/D, Space, Escape, the four arrow keys, Tab, F5, and F9, Q/E/R/F, N/C, Z/Y, left/right Control, and
Delete.

| Method | Behavior |
| --- | --- |
| `press(key)` | Add a press transition only when the key was not already held |
| `release(key)` | Add a release transition only when the key was held |
| `is_held(key)` | Query persistent held state |
| `just_pressed(key)` / `just_released(key)` | Query transitions since the last frame clear |
| `end_frame()` | Clear transitions while keeping held keys |
| `reset()` | Clear held keys and both transition sets |

A press/release within one frame can set both transitions; repeated key-down events do not create a new transition while
held. The game maps physical winit key codes into this enum, runs update, then calls `end_frame()`. Focus loss clears
input and releases mouse capture. The game now routes native panel text, clicks, and drags separately and suppresses
held movement during editing contexts. Key rebinding, general action mapping, and gamepad support are not provided by
InputState itself.

## camera.rs: Camera

`runtime::Camera` stores public position, yaw, pitch, vertical FOV, near, and far values. Defaults are position `[0, 0, 3]`, yaw/pitch zero, 60-degree FOV in radians, near 0.1, and far 100.

`forward()` gives the view direction (negative Z at default orientation). `right()` gives a horizontal yaw-dependent direction; `target()` is position plus forward. `rotate(yaw_delta, pitch_delta)` wraps yaw into one turn and clamps pitch to ±89 degrees. Angles/deltas are in radians. Public field writes bypass this rotation clamp; projection validation is handled by SceneData on scene-file load; a reusable CameraController now lives in its own module.

This CPU camera is distinct from `renderer::vulkan::VulkanCamera`, which owns GPU uniform resources. The game uses CPU camera values and the prepared surface aspect ratio to build `CameraUniform::perspective`, then passes it to `WindowRenderer::draw`.

## camera_controller.rs — in progress

`runtime::CameraController` separates camera behavior from native event handling. `update(&mut camera, &input, delta_seconds)` applies normalized arrow-key movement using camera forward/right; default move_speed is 2 units/second. `mouse_motion(&mut camera, dx, dy)` applies default sensitivity 0.002 to displacement without multiplying by frame time. The game still decides when focus/capture permits motion. This is a prototype controller, not action mapping or an editor camera framework.

Generic selection/commands now live in runtime SceneEditor; RobotGame owns optional simulation during Play. GameApp
starts in Edit, routes panels/drag/native events, manages scene I/O and temporary Play state, updates CameraController,
and delegates edits or simulation according to mode. See [scene save/reload](scene-persistence.md).

## Current development controls — provisional

| Input                 | Current behavior                                                         |
|-----------------------|--------------------------------------------------------------------------|
| W/S                   | Move Edit selection in world Y; move robot_a in robot Play               |
| A/D                   | Move Edit selection in world X; move robot_a in robot Play               |
| Arrow keys            | Move camera forward/backward and strafe right/left                       |
| Right mouse press     | Capture cursor for mouse look while focused                              |
| Captured mouse motion | Rotate yaw/pitch; sensitivity currently 0.002 radians per motion unit    |
| Escape                | Release capture and restore cursor visibility                            |
| Space                 | Toggle robot simulation pause during Play (rotation and WASD)            |
| Tab                   | Cycle all scene entities                                                 |
| Q/E, R/F              | Rotate Z in 5-degree steps; scale ×/÷1.1                                 |
| Ctrl+Z / Ctrl+Y       | Mixed Edit transform/name/entity undo / redo                             |
| N / C / Delete        | Undoable Edit spawn / duplicate / delete                                 |
| F1 / F6 / F7          | Toggle panels / Edit-Play / culling                                      |
| 1 / 2 / 3             | Translate / rotate / scale gizmo in Edit                                 |
| Left click / drag     | Panel/field action, handle drag, or ray selection in Edit                |
| F5 / F9               | Save / reload the active JSON scene; reload wins if both arrive together |

Object speed is currently 1 world unit/second; camera speed is 2. Diagonal input is normalized. Camera forward motion follows pitch, so looking up/down also changes the vertical movement component. Mouse motion is displacement and is deliberately **not** multiplied by frame delta.

Capture attempts Locked mode then Confined as fallback, and only enables mouse look after success. Capture errors are logged. Focus loss, zero-size resize, and application exit release capture; right-button release is not currently the release gesture. Installed/name-file mode skips movement/rotation updates and disallows capture.

## Checks and remaining verification

```powershell
cargo test -p runtime --lib --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --offline --locked
```

Unit tests cover clock precision/capping/reset/backwards timestamps, key transitions/repeats/reset, camera direction math, yaw wrapping, pitch limits, and reusable controller movement/mouse math. Game tests cover selection and robot behavior; scene-I/O checks are described separately. They do not prove native cursor capture, focus handling, perceived speed, or movement during minimize/restore.

Before declaring this milestone complete, exercise the provisional controls visibly: verify diagonal speed, held/released keys, focus loss, capture fallback/error behavior, Escape release, minimized/resumed timing, and camera orientation. Fixed-step gameplay, binding configuration, and broader input devices remain future design work. See [testing](testing.md) and [pipeline](pipeline.md).
