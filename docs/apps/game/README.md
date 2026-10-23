# Native game and editing application

Source: `apps/game`. The application now provides a native Edit/Play prototype built on shared runtime editing APIs. It
is distinct from the Angular/Tauri editor. Steps 11–20 are implemented with automated verification; visual/native
interaction acceptance is still limited. Read the [change report](../../editor-steps-11-20.md) for exact files,
examples, and evidence.

## Run and modes

From the repository root after [setup](../../usage.md):

```powershell
cargo run -p game --offline --locked
.\dev.ps1 game
```

The final ordinary-target build encountered Windows access/file-lock error 5 replacing ``target/debug/game.exe``. An
isolated build passed at ``target/steps-11-20-verification/debug/game.exe``; use
``cargo run -p game --locked --offline --target-dir target/steps-11-20-verification`` for that location. No process was
terminated, and the default output is not claimed refreshed.

Vulkan 1.3, a capable driver, shader compiler, and debug validation layers are required. The saved scene remains
`apps/game/assets/scenes/robots.json`; its current content is user-authored and is not reset to default transforms.

Development starts in Edit, loading named keyed entities/camera through runtime scene I/O. F6 clones authored
scene/camera into temporary Play state with shared model resources; Stop restores authored data. RobotGame supplies
optional simulation only when suitable role bindings exist. A generic scene need not contain robots. Installed/name-file
mode uses executable-relative assets and does not expose development editing panels/update tools.

## Architecture

- `main.rs`: window/event routing, GPU lifecycle, Edit/Play, scene I/O, draw submission.
- `game.rs`: robot-specific simulation during Play.
- `editor_tools.rs`: fields, dirty state, asset placement, drag lifecycle, CPU statistics.
- `editor_panel.rs`: flat entity list, properties, assets, and status controls.
- `selection_overlay.rs` / `gizmo_overlay.rs`: cached bounds box and batched gizmo rendering.
- `runtime::SceneEditor`: reusable picking, selection, edits, and mixed history.
- `renderer::WindowRenderer`: uploads, dynamic-buffer synchronization, presentation/resize/shutdown.

GameApp owns scene/cache/editor/tool state before the renderer in destruction order. GPU idle waits still precede
releasing externally uploaded resources. Arc sharing and application field order do not create a backend-independent
resource lifetime system.

## Controls and content

F1 toggles panels, F5 saves authored content, F6 toggles Play/Edit, F7 toggles culling, and F9 reloads. In Edit, left
click selects objects/fields/handles; 1/2/3 choose Translate/Rotate/Scale, Tab cycles entities, WASD/QE/RF edit
transforms, N/C/Delete perform undoable structural edits, and Ctrl+Z/Y use mixed history. In robot Play, Space pauses
robot simulation (rotation and WASD); otherwise WASD moves robot_a. Arrow keys and captured right-mouse motion control
the camera. Text entry and drag modes suppress conflicting movement.

Property rotations display degrees and store radians. Release commits a drag as one edit; Escape/focus/resize/mode
transitions cancel it. Asset Refresh/Place discovers GLB/glTF under the root and places a shared model three units
forward. The dirty marker reflects authored saved-content comparison, including camera. F5 in Play saves authored
content, not simulation.

## Rendering and limits

Model-local bounds support box picking and selection overlays. Scene rendering uses frustum culling with conservative
AABBs and exposes entity/draw/triangle counts. Objects now have area-weighted/imported normals and basic fixed
directional lighting; editor overlays are unlit. Panel FRAME/UPDATE values are averaged wall frame intervals/update
duration, not GPU timings.

The bitmap panel has ASCII display/fallback glyphs and compact-window constraints. Picking is not triangle-accurate.
Full PBR, shadows, hierarchy parenting, glTF node traversal, animation/skinning, and Angular editor integration remain
future work.

The compiler recursively copies the complete scene/model/texture asset tree. The admin source generator still omits
scene JSON from its allowlist, so generated compilation alone does not certify runnable scene startup.

## Checks

```powershell
cargo check --workspace --all-targets --locked --offline
cargo test -p runtime -p game -p renderer --all-targets --locked --offline
```

The implementation/tester handoff reports 183 passes, zero failures, and one ignored native initialization test for the
standard suites. See [testing](../../testing.md) and the [change report](../../editor-steps-11-20.md) for supplementary
evidence and native visual limits. Historical renderer smoke results do not establish appearance of these new
panels/gizmos/lights.
