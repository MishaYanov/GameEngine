# Milestone 10 camera authoring and parity

The editor can author Fixed, Follow and Orbit cameras. Orbit preview lets you try a draft against the scene's colliders
before you apply it. The Camera Acceptance Room exercises the same camera definition in editor Play, the standalone
player and a headless replay.

## Try it in the editor

Run the following from the repository root in PowerShell. The compiler creates a project named **Camera Acceptance Room
** inside the destination. The editor opens its generated `project.json`. Staging also gives you an independent copy to
edit.

```powershell
$cameraParent = Join-Path $env:TEMP ("camera-review-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $cameraParent | Out-Null
cargo run --locked --offline -p compiler -- --stage apps/game/camera-demo.json $cameraParent
$cameraRoom = Join-Path $cameraParent 'Camera Acceptance Room'
Join-Path $cameraRoom 'project.json'
.\dev.ps1 editor
```

1. Choose **Open Project…** and open the printed manifest. In **Game Camera…**, select **Orbit**. The room targets the
   stable entity key `player`, starts at 5 m and uses a 0.25 m probe. Distances are in world units. The form displays
   angles in degrees and writes radians.
2. Change the initial distance and choose **Preview Orbit**. The toolbar replaces the modal so you can see the native
   viewport. Use **Turn right**, **Zoom in** or **Advance one step**. Each accepted sample advances the camera against
   frozen scene geometry. Gameplay and animation clocks stay at rest.
3. Choose **Back to settings** to continue editing that draft, or **Close preview** to discard it. **Apply Game Camera**
   submits one revision-checked edit. Undo and Redo act on the whole camera. Save All and reopen to check the authored
   defaults.
4. Use the original 5 m setting to reproduce the table below. Enter Play, select **Game**, and click inside the image to
   capture input. Hold **S** for one second, then release for three seconds. The rear wall shortens the arm. Hold **D**
   for one second, then release for four seconds to watch the arm recover. Mouse motion orbits, the wheel zooms and
   Escape releases capture. Pause, resume and Restart Run exercise the rig's input lifetime.
5. Stop Play to return to authoring. In a window wider than 3:1, Game and preview use centered content with black side
   bars. The bar area lies outside Game pointer input. Scene navigation uses the full editing viewport.

For a standalone build, repeat the compiler command with a fresh existing parent and `--export` in place of `--stage`.
Launch the generated player from that directory.

```powershell
$cameraBuild = Join-Path $env:TEMP ("camera-export-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $cameraBuild | Out-Null
cargo run --locked --offline -p compiler -- --export apps/game/camera-demo.json $cameraBuild
& (Join-Path $cameraBuild 'Camera Acceptance Room/game.exe') --player
```

Add `--smoke-test` to the player command to draw one hidden frame and exit on the executable's normal main thread.

## Replay and read a checkpoint

```powershell
$wall = cargo run --quiet --locked --offline -p engine-headless -- --project apps/game/camera-demo.json --replay history/verification/milestone-10-camera-wall.json | ConvertFrom-Json
$wall.camera_runtime.orbit | Select-Object user_distance,effective_distance,obstruction,tick
cargo test --locked --offline -p engine-headless --test camera_replay
```

Version 2 replays accept bounded yaw, pitch and logarithmic distance deltas. The headless summary's
`camera_runtime_hash` is a 64-bit FNV-1a checksum over a size-limited, typed `camera_runtime_v1` JSON value. It includes
the pose, target generation, whether the target belongs to the current world, pivot, settling and obstruction state, and
unconsumed input. Edit mode hashes JSON `null`. Compare equal input boundaries on the same build and platform. The
scene/project and native-component checksums retain their own
scopes. [Wall replay](../verification/milestone-10-camera-wall.json) · [Full route](../verification/milestone-10-camera-route.json)

| Checkpoint                   | Observed result in the 60 Hz room                                                                             |
|------------------------------|---------------------------------------------------------------------------------------------------------------|
| Admission                    | The rig targets the player at 5 m. The intervening sensor allows a clear arm.                                 |
| Rear wall, tick 240          | `camera-wall` holds the effective distance at 3.7768276 m. Requested distance stays at 5 m.                   |
| Moving clear, tick 300       | The obstruction clears. The arm is still recovering at 4.8995957 m.                                           |
| After moving right, tick 540 | The blocker clears and the arm settles at 5 m.                                                                |
| Queued turn before a step    | The camera checksum changes. The eye pose and the other two checksum scopes stay at the preceding checkpoint. |
| Next fixed step              | Yaw reaches 0.25 radians and the requested distance reaches 4.524187 m.                                       |
| Restart                      | Camera projection, native-component projection and scene/project checksum match fresh admission.              |

The process test compares every checkpoint through the source manifest and a newly staged project. A separate release
run also compared the installed manifest. Four fixed steps per host tick gave the same result as one per tick with the
same input boundaries. Fixture tests cover the sensor, both doorposts, the opening, the lintel and the rotated collider.
The rotated collider uses its conservative world-axis-aligned
bounds. [Verification ledger](../verification/milestone-10-camera-results.json)

## Native rendering and performance

In the native editor, previewing a 6 m draft left the authored revision at zero. Apply, Undo, Redo, save and reopen
restored the edited 6 m setting. Play showed the rear wall's entity key and the recovery message as the character moved.
Pause, Resume, Restart Run and Stop operated on the real Game view. Framebuffer captures covered 1072×603, 497×886 and
1587×332. The ultrawide image had 996 content pixels per row between two black bars measuring 295 and 296 pixels. QA
exercised the movement route through the editor bridge on a private Windows desktop. Windows kept the background
viewport unfocused when it received keyboard messages. A hands-on foreground mouse-capture check remains open.

On a Windows PC with an i7-13700K and RTX 4070, the exported executable initialized and drew through its ordinary main
thread. A hidden native player test supplied keyboard state and camera intent to the installed camera room at 1280×720
and 1920×1080. It matched five camera projections against release headless. A separate framebuffer test used the bundled
Courtyard scene and HUD. All 196,608 content pixels were identical between 768×256 and the centered content of 1024×256.
All 65,536 bar pixels were black. The portrait case also rendered geometry and the HUD.

Across three release runs, the entire headless fixed step took 5.9–6.1 µs at the median and 6.5–6.9 µs at p95 while the
animated player moved laterally and the wall blocked the camera. For a pure query over 1,000 eligible obstacles, the
median of 31 batch means was 26–49 µs per cast, with 32 casts per batch after warm-up. A scan of 8,193 filtered entries
rejected when it exhausted the 8,192-visit allowance. Repeated preview close returned the frozen document reference
count to baseline in 64 cycles. The ledger separates update, GPU and update-through-present samples. The last metric
includes presentation waits. [Machine results and fixture fingerprints](../verification/milestone-10-camera-results.json)

Run the release measurements locally with cached dependencies. The GPU framing test uses the Vulkan SDK screenshot layer
on Windows.

```powershell
cargo test --release --locked --offline -p engine-headless -p engine-core --lib --bins measure_camera -- --ignored --nocapture --test-threads=1
cargo build --release --locked --offline -p engine-headless
$previousCameraProject = $env:GAME_ENGINE_QA_CAMERA_PROJECT
try {
    $env:GAME_ENGINE_QA_CAMERA_PROJECT = Join-Path $cameraBuild 'Camera Acceptance Room/project.json'
    cargo test --release --locked --offline -p game --bin game camera_native_tests -- --ignored --nocapture --test-threads=1
} finally {
    $env:GAME_ENGINE_QA_CAMERA_PROJECT = $previousCameraProject
}
```
