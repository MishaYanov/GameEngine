# Timing, input, and camera

The Editor and standalone Player use shared fixed-step simulation, authored input layouts and camera behavior.
The native viewport routes input according to focus and the active Scene or Game view. Use the Controls menu
to author bindings and conditional layouts for the current project.

## Orbit camera obstruction

An authored `camera.orbit` follows an entity by key. The **camera arm** is the segment from its filtered pivot
to the requested eye position. On admission and on each resolved fixed step, the rig probes the full requested arm
against the current world colliders. A wall moving into the arm pulls the eye in on that same step, even below the
authored minimum zoom distance. The rig keeps the requested zoom so the eye can return when the wall clears.
`recovery_seconds` controls that outward movement. Set it to zero for immediate recovery.

For example, place the pivot at the origin looking along −Z, request a 5-unit arm, and put a box wall at
Z = 3 with a half-depth of 0.25. Its near face is at 2.75. A `probe_radius` of 0.25 limits the eye to just
under Z = 2.5 after clearance and rounding. A solid collider on a matching layer still blocks when its own
detection mask is zero. A trigger in exactly the same position leaves the arm clear.

| Setting or scene property           | Effect on the probe                                                                                                                                                                                                                                                   |
|-------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `probe_radius`                      | Covers all four near-plane corners for a Game aspect ratio up to 3:1. For a near plane of 0.1 and a vertical FOV of 60°, the minimum is about 0.20917 units including the 0.001 margin. A radius of 0.25 passes. Widening the projection can require a larger radius. |
| `obstruction_mask`                  | A wall blocks if any of its layer bits overlap this authored nonzero mask. Enable its collider and clear its sensor flag. Mesh visibility has no effect.                                                                                                              |
| Target and descendants              | The probe skips the target's whole subtree, including attached colliders. A topology change invalidates the cached exclusion indices.                                                                                                                                 |
| `pivot_offset` and `follow_seconds` | Place the pivot sphere in clear space and allow room for follow lag. A pivot strictly inside an expanded obstacle produces an error naming its entity key.                                                                                                            |

### Runtime behavior and limits

The query sweeps the eye sphere conservatively against each collider's enclosing world-axis-aligned box.
Rounded shapes and rotated walls can therefore shorten the arm early. Contact may shorten it to zero.
The guarantee applies along the pivot-to-eye segment of each accepted pose. A large yaw change can move between
two accepted poses around a corner without checking the arc between those poses.

The probe measures collider faces relative to the pivot before expanding them by the radius. This preserves a
small overlap even when the absolute coordinates are large enough to round away the radius. It also checks the
eye after rounding to the position the camera can store. A projection that exceeds camera precision can still fail.

The query reads synchronized collision geometry without moving bodies or generating trigger events. Equal hit
fractions select the lexicographically lowest blocker key. Each cast permits 8,192 work visits, including skipped
candidates and endpoint checks, and draws from the 1,000,000 collision candidates shared across the tick's fixed
steps. Rebuilding exclusions also consumes that shared budget. If a cast rejects the second step of a multi-step
tick, the session restores the world and camera from before the tick, clears live input and pauses.

Entering Play, restarting and preparing a destination scene resolve the first camera before publication.
A failed preparation preserves the current run. Runtime prefab insertion checks the new layout before committing
the spawn. The CPU-only `FrozenOrbitPreview` uses the same query against a frozen scene and exposes the accepted
pose and orbit state. Use `user_distance`, `last_allowed_distance`, `effective_distance` and
`obstruction.blocker` to distinguish a zoom request, the current safe limit, the displayed arm, and the wall
that set the limit. The editor's **Preview Orbit** action opens this frozen rig and samples it from a toolbar above the
native viewport.
[Edit and preview the game camera](viewport-implementation.md).
[Run the camera regression tests](testing.md#orbit-camera-obstruction-checks).

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

## Authored game input

Open **Controls** and use **Controls and Input Layouts…** to assign actions for the current project. A layout
maps device inputs to named actions. Choose a default layout and add a conditional layout for a scene or a
grounded state. The runtime takes the first matching conditional layout and falls back to the default.
Use **Save All** to persist the project settings.

Editor Play and Player sample keyboard, pointer and supported gamepad inputs through the shared action
path. The current local player receives the ordered samples for its possession epoch. Possession handoff,
focus loss, Stop and scene replacement clear held state. [Project settings](project-lifecycle.md) ·
[Replay checks](testing.md#headless-replay).

## camera.rs: Camera

`runtime::Camera` stores public position, yaw, pitch, vertical FOV, near, and far values. Defaults are position `[0, 0, 3]`, yaw/pitch zero, 60-degree FOV in radians, near 0.1, and far 100.

`forward()` gives the view direction (negative Z at default orientation). `right()` gives a horizontal yaw-dependent direction; `target()` is position plus forward. `rotate(yaw_delta, pitch_delta)` wraps yaw into one turn and clamps pitch to ±89 degrees. Angles/deltas are in radians. Public field writes bypass this rotation clamp; projection validation is handled by SceneData on scene-file load; a reusable CameraController now lives in its own module.

This CPU camera is distinct from `renderer::vulkan::VulkanCamera`, which owns GPU uniform resources. The game uses CPU camera values and the prepared surface aspect ratio to build `CameraUniform::perspective`, then passes it to `WindowRenderer::draw`.

## camera_controller.rs — in progress

`runtime::CameraController` separates camera behavior from native event handling. `update(&mut camera, &input, delta_seconds)` applies normalized arrow-key movement using camera forward/right; default move_speed is 2 units/second. `mouse_motion(&mut camera, dx, dy)` applies default sensitivity 0.002 to displacement without multiplying by frame time. The game still decides when focus/capture permits motion. This is a prototype controller, not action mapping or an editor camera framework.

Generic selection/commands now live in runtime SceneEditor; RobotGame owns optional simulation during Play. GameApp
starts in Edit, routes panels/drag/native events, manages scene I/O and temporary Play state, updates CameraController,
and delegates edits or simulation according to mode. See [scene save/reload](scene-persistence.md).

## Current editor intent versus legacy controls

The new Angular editor contract does not assign physical keys. EditorSession offers frame/orbit/pan/dolly, snapping,
multi-key transforms and Play input as typed commands; its capability descriptors tell UI owners what is available. A UI
owner chooses how raw viewport events map to intent. The current Angular workspace composes the controls and native
pane, while focused native Scene tools own spatial shortcuts and Game routes authored input bindings.

The earlier WASD/Tab/QE/RF/F-key native demonstration controls apply only when GAME_ENGINE_NATIVE_EDITOR_DEMO=1, not
normal game launch or default EditorSession policy. Normal game mode is overlay-free.
See [Editor commands](scene-editing.md) for authoring settings and [Editor viewport controls](viewport-implementation.md) for desktop bindings.

## Fixed simulation step

`runtime::simulation::FixedStepper` defaults to60Hz and at most8 catch-up steps per advance. It caps accepted elapsed
time at0.25seconds, reports dropped time and interpolation, and resets on pause. This is separate from FrameClock's
capped redraw delta and FIFO display pacing. Editor Play ticks apply validated movement intent, not keyboard codes;
conservative collision can block motion or pause/reset after error.

See [Editor viewport controls](viewport-implementation.md) for camera gesture and framing examples.

## Checks and remaining verification

```powershell
cargo test -p runtime --lib --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --offline --locked
```

Unit tests cover clock precision/capping/reset/backwards timestamps, key transitions/repeats/reset, camera direction math, yaw wrapping, pitch limits, and reusable controller movement/mouse math. Game tests cover selection and robot behavior; scene-I/O checks are described separately. They do not prove native cursor capture, focus handling, perceived speed, or movement during minimize/restore.

Visible acceptance should still verify diagonal speed, held/released keys, focus loss, capture fallback/error behavior,
Escape release, minimized/resumed timing and camera orientation. Exercise a physical controller when changing device
routing or reconnect behavior. See [testing](testing.md) and [pipeline](pipeline.md).
