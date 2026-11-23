# Primitive Workshop

This headless test project uses the normal Editor and Player asset paths. Work from a copy when trying the fixture in the Editor.

## Try the objective

1. Open the copy's `project.json` from **File → Open Project…**. Expand **Floor** in the Hierarchy to find the Plane and its separate box collider. Select the Capsule Player, then its visual child, to inspect the controller and mesh independently.
2. Open the Workshop Gate Prop from **Assets**. Its Signal Cap is a nested Prop with a Sphere mesh. The Gate Pivot carries the registered `project.open_door_on_trigger` script and refers to the Gate Sensor in the same assembly.
3. Select **Play**. Move with the arrow keys and jump with **J**. Hold Right Arrow until the Gate Sensor is reached. The gate swings open. The Workshop Bell plays a short clip when its animation reaches the `bell-contact` marker.
4. Select **Restart**. The player returns to the start and the gate closes. Select **Stop** to return to the authored scene.

## Recreate the scene from Blank

1. Use **Create → Plane** at the origin, scale it to `(12, 1, 10)`, and name it **Plane Floor**. Create an empty object at `(0, -0.1, 0)` and add a Box Collider with half extents `(6, 0.1, 5)`. Put both root objects in a folder named **Floor**.
2. Set up a Player Controller and a Character Controller with a box collider on the Player layer. Use a Capsule visual child scaled to `(0.7, 0.9, 0.7)`. Keep the controller root unscaled and place it at `(0, 0.901, 0)`. Assign the player binding and make an orbit Camera targeting the player the scene's game camera.
3. In **Controls**, bind Move to the arrow keys, Look to pointer motion, and Jump to **J**. Use the Move, Look, and Jump outputs in the Controls Graph and attach the graph to the Player Controller. The exact asset definitions are in `assets/controls`.
4. Make a Sphere Signal Cap Prop. Nest it in a Workshop Gate Prop that contains the hinge, pivot, and sensor. Use the trigger script settings in `assets/prefabs/workshop-gate.prefab.json`. In **Physics Layers**, allow Player–Trigger interaction. Place a linked Workshop Gate instance at `(3.2, 0, 0)`.
5. Give the Workshop Bell a visual child and a one-shot Animator state using an authored rotation clip. Add `bell-contact` at tick 450. In the Audio Source, use **On Event** with the clip and marker as the trigger. The fixture uses the existing practice-yard bell waveform as a local WAV asset.
6. Save All, reopen the project, and try the objective. Use **Export Game…** to stage the same scene for the Player.

## Automated coverage

The integration test stages the project with the compiler, then runs the source and staged fixtures from an unrelated directory. It checks the normal GLB files, floor folder, player and camera bindings, nested Prop, gate movement, committed audio marker, and restart. It also checks that replay leaves the source scene intact. Run the test from the repository root.

```powershell
cargo test --locked --offline -p engine-headless --test primitive_workshop
```

[Object authoring roadmap](../../../../../designs/editor-viewport/object-authoring-roadmap-163-202.md#190-build-one-playable-workshop-from-a-blank-project)
