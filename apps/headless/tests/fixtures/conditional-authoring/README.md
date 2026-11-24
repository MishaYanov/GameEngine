# Conditional Door Lab

## Try it in the Editor

1. Copy this folder so that you can save changes in your own copy. In the desktop Editor, choose **File → Open Project…**
   and select the copy's `project.json`. The scene opens with a Player Controller and a floor on the Terrain layer.
2. In **Assets**, select the Prop at `prefabs/trigger-door.prefab.json`. Click **Place Linked Prop…**, enter
   `(2.4, 0, 0)` for **Placement position**, then select **Place Linked Instance**. Repeat at `(8, 0, 0)`.
3. Open **Controls → Input Actions Assets** and inspect the Land and Airborne layouts. Use **Game → Physics Layers**
   to find the Player–Trigger cell. The project starts with this pair set to Allow.
4. Select **Play**. With the Player on the floor, press Space to jump. Release Space, then press it again before landing
   to send Attack. After landing, hold Right Arrow to enter the first sensor, then release when the first door opens.
   The second door stays closed until the Player enters its own sensor.
5. Stop Play, set the Player–Trigger cell to Ignore and apply the change. Play again and walk into the first sensor.
   The door stays closed. Undo the matrix edit to restore the interaction.

**Resources.** See the [authoring roadmap](../../../../../history/roadmaps/authoring-roadmap-134-153.md) for
the input conditions, script registration and collision rules. The test below provides a repeatable route.

## Automated checks

The test places both linked doors through the editor authoring API and saves the resulting scene in a temporary project.
It compares source and staged Player data from the same replay. The checks cover single and grouped fixed steps,
accepted actions, sensor events, door rotations, stopping and restarting Play, and native export preflight. The
negative cases verify an ignored collider pair and an unregistered script.

Run from the repository root.

```powershell
cargo test --offline --locked -p engine-headless --test conditional_authoring
```

Run the opt-in Player startup check on a Windows or Linux machine with the engine toolchain and a Vulkan driver.

```powershell
$env:CARGO_NET_OFFLINE = 'true'
cargo test --locked -p engine-headless --test conditional_authoring exported_conditional_world -- --ignored
```
