# Controller Practice Yard

For the full Bell Gate, audio, Sky, Check Scenes, and Windows export exercise, follow the [Practice Yard walkthrough](practice-yard.md) and open the [full yard project](full/project.json).

This test project uses a scene-root Player, a separate Orbit Camera, a camera obstruction wall, and a step higher than the motor can climb without jumping. The Player’s visual objects are children of the motor root. The Arrow keys, J, and F are deliberate asset bindings that exercise Input Actions independently of the project's fallback controls.

1. Copy this directory to a disposable project location and open `project.json` in the editor. Select **Player** and confirm that Player Controller owns local input and uses `controls/yard.input-actions.json`. Select **Orbit Camera** and confirm that it is the Game Camera and targets **Player**.
2. In Play, move right with **Right Arrow** or the left stick until the view clears the wall. Move toward the step with **Up Arrow** and jump using **J** or the bottom face button. Rotate the view with the pointer or the right stick. **F** and the left face button send the `attack` action.
3. Pause and resume using the editor toolbar. Stop, and check that the Player and Camera are back at the authored positions. Start again and verify that movement and camera input begin neutral.
4. Export the project. From a different directory, start the exported Player and repeat the route. The fixture uses a single small reusable cube model for all visible geometry.

```powershell
cargo test -p engine-headless --offline --locked --test controller_replay practice_yard
cargo test -p engine-headless --offline --locked --test camera_replay practice_yard
$project = (Resolve-Path apps/headless/tests/fixtures/authoring-controller/project.json).Path
$exportRoot = (New-Item -ItemType Directory -Force target/controller-lab-export).FullName
cargo run -p compiler --offline --locked -- --export $project $exportRoot
```

The [replay fixture](practice-yard.replay.json) labels the start, obstruction, recovery, jump, landing, pause and restart. The [scene](assets/scenes/practice-yard.json) and [Input Actions](assets/controls/yard.input-actions.json) are available for inspection and later Controls Graph work.


## Controller and Animator route

Open [graph-project.json](graph-project.json) to use the graph-wired Practice Yard. The Player assigns [yard.controls.json](assets/controls/yard.controls.json) and has [yard.animator.json](assets/animations/yard.animator.json) attached. The Animator drives the Training Blade visual child through the [Training Blade Prop](assets/prefabs/training-blade.prefab.json) channel. The four states pin local-position clips for Idle, Walk, Jump and Practice Swing. The fixture’s authored gameplay rule accepts Attack only for the local Player. The same routed action supplies the held Aim Boolean.

1. Open the graph project from a copy of the fixture. Select **Player** in the Hierarchy. Use **Open Controls Graph** under the Player Controller, and **Open Animator** under Animator.
2. Open the Training Blade Prop and review the [swing clip](assets/animations/yard-swing.clip.json) in Animation. Move the middle position key, choose **Discard**, and reopen the clip. The midpoint key and bookmark should still be at 150 ms.
3. Enter Game with the Player selected. After a short idle, hold **Right Arrow** or the left stick, then press **J** or the bottom face button once. Let the Player land, then press **F** or the left face button once. Open the Runtime Inspector and refresh to see the live graph route and Animator state. Hold the swing control and confirm it produces one accepted press.
4. Restart Play, then stop. Confirm Idle and neutral controls after restart and the authored Training Blade pose after Stop. Export from the copy and do the same keyboard and gamepad checks in the Windows Player.

```powershell
cargo test --offline --locked -p engine-headless --test authoring_graph_replay
cargo test --offline --locked -p app --lib practice_yard_graphs_reopen_and_discarding_a_prop_clip_edit_preserves_play
$graphProject = (Resolve-Path apps/headless/tests/fixtures/authoring-controller/graph-project.json).Path
$graphExport = (New-Item -ItemType Directory -Force target/controller-graph-export).FullName
$env:GAME_ENGINE_QA_CONTROLLER_PROJECT = $graphProject
cargo test --offline --locked -p game --bin game practice_yard_player_moves_jumps_orbits_and_restarts -- --ignored
cargo run --offline --locked -p compiler -- --export $graphProject $graphExport
```

The [graph replay](graph-route.replay.json) compares accepted presses, motion, camera, transition IDs and visual world matrices with a staged copy opened from another directory. These tests use fixed-step inputs and a data-only export stage. Record Windows editor and Player visuals with a physical keyboard and gamepad separately.

## Resources

- [Controller replay tests](../../controller_replay.rs)
- [Camera replay tests](../../camera_replay.rs)
- [Existing Cinder Oath native regression](../../../../game/tests/unit/tests_camera_native.rs)

- [Graph fixture replay and build failure checks](../../authoring_graph_replay.rs)
- [Editor Play and canceled clip test](../../../../editor-host/src-tauri/tests/unit/animation_preview.rs)
- [Graph replay projection](../../../src/report/graph.rs)
