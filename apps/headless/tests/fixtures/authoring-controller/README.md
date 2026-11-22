# Controller Practice Yard

This test project uses a scene-root Player, a separate Orbit Camera, a camera obstruction wall, and a step higher than the motor can climb without jumping. The Player’s visual objects are children of the motor root. The Arrow keys, J, and F are deliberate asset bindings that exercise Input Actions independently of the project's fallback controls.

1. Copy this directory to a disposable project location and open `project.json` in the editor. Select **Player** and confirm that Player Controller owns local input and uses `controls/yard.input-actions.json`. Select **Orbit Camera** and confirm that it is the Game Camera and targets **Player**.
2. In Play, move right with **Right Arrow** or the left stick until the view clears the wall. Move toward the step with **Up Arrow** and jump using **J** or the bottom face button. Rotate the view with the pointer or the right stick. **F** and the left face button send the `attack` action.
3. Pause and resume using the editor toolbar. Stop, and check that the Player and Camera are back at the authored positions. Start again and verify that movement and camera input begin neutral.
4. Export the project. From a different directory, start the exported Player and repeat the route. The fixture uses a single small reusable cube model for all visible geometry.

```powershell
cargo test -p engine-headless --offline --locked --test controller_replay practice_yard
cargo test -p engine-headless --offline --locked --test camera_replay practice_yard
cargo run -p compiler --offline --locked -- --export apps/headless/tests/fixtures/authoring-controller/project.json target/controller-lab-export
```

The [replay fixture](practice-yard.replay.json) labels the start, obstruction, recovery, jump, landing, pause and restart. The [scene](assets/scenes/practice-yard.json) and [Input Actions](assets/controls/yard.input-actions.json) are available for inspection and later Controls Graph work.

## Resources

- [Controller replay tests](../../controller_replay.rs)
- [Camera replay tests](../../camera_replay.rs)
- [Existing Cinder Oath native regression](../../../../game/tests/unit/tests_camera_native.rs)
