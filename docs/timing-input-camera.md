# Timing, input, and camera

The low-level clock/key/camera APIs remain shared. Core fixed-step simulation, authored project input profiles,
the Angular Game Input Manager and native Scene/Game focus routing build on them. Older native-demo details below
retain their milestone context; the [building-blocks review](building-blocks-review.md) records current input,
course/restart and verification scope. Automated and hidden native focus tests remain distinct from visible desktop
interaction.

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
[Author, preview and play the camera room](milestone-10-camera-review.md).
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

Project input profiles map four movement directions and bounded named actions to serialized physical key names.
The Game Input Manager supplies binding selection/key capture and a held-key movement/action preview. Apply uses
the current reviewed workspace generation/revision and preserves unrelated current project settings; a changed
input profile requires reopening/review rather than overwriting it. Save All/Save Project persists the accepted
profile. Scene/project replacement and late responses cannot apply stale settings or feedback to a new workspace.

Core resolves native codes and Player keys through the same input profile. Focus loss and the existing pause,
mode, scene activation and restart boundaries clear or suppress held input. This is configured keyboard routing,
not a general gamepad/possession system or animation player. [Input persistence review](building-blocks-review.md#input-and-project-authoring).

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
See [current editor guide](scene-editing.md) and the historical [steps11–20](editor-steps-11-20.md) rather than treating
old key examples as current product bindings.

## Fixed simulation step

`runtime::simulation::FixedStepper` defaults to60Hz and at most8 catch-up steps per advance. It caps accepted elapsed
time at0.25seconds, reports dropped time and interpolation, and resets on pause. This is separate from FrameClock's
capped redraw delta and FIFO display pacing. Editor Play ticks apply validated movement intent, not keyboard codes;
conservative collision can block motion or pause/reset after error.

See [steps23–25 and32](editor-steps-21-35.md#step-23-frame-orbit-pan-and-dolly-camera-commands) for concrete
command/settings examples. Fixed stepping does not establish networking determinism or a complete physics engine.

## Checks and remaining verification

```powershell
cargo test -p runtime --lib --offline --locked
cargo check -p game --all-targets --offline --locked
cargo run -p game --offline --locked
```

Unit tests cover clock precision/capping/reset/backwards timestamps, key transitions/repeats/reset, camera direction math, yaw wrapping, pitch limits, and reusable controller movement/mouse math. Game tests cover selection and robot behavior; scene-I/O checks are described separately. They do not prove native cursor capture, focus handling, perceived speed, or movement during minimize/restore.

Visible acceptance should still verify diagonal speed, held/released keys, focus loss, capture fallback/error behavior,
Escape release, minimized/resumed timing and camera orientation. Fixed-step gameplay and keyboard binding
configuration are implemented; broader input devices remain future work. See [testing](testing.md) and [pipeline](pipeline.md).
