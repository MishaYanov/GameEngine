# The Hollow Maze

A turn-based chase puzzle. Lead the knight through an open-air maze under an ember-red sky, collect all three colored
rune seals, and cross the opened gate. The horned Warden advances toward you after every second turn. A blocked walk
costs no turn. Low brass barriers stop walking and the Warden’s pursuit. If you cross paths with the Warden, the run is
over.

## Play

Open this folder's `project.json` in an Editor built from this checkout. Click **Play** and focus the Game viewport.

- **WASD** moves one tile per press. **Left click** or **E** steps in the direction the knight is facing.
- **Space** or **right click** jumps in the direction the knight is facing. Hold a movement key while jumping to choose
  another direction. Jumping over a low barrier lands on the next open tile.
- **Q** waits a turn in place. The Warden counts jumps and waits as turns.
- **Restart Run** starts a fresh run in the Editor. The standalone Player uses **F10** after victory or defeat.

To build a standalone game, stop Play, save changes, then use **Export Game…** in the workspace controls and choose an
existing destination folder. The export contains the Player, local project data and all referenced assets.

## Project files

- `project.json` stores the gameplay bindings, tile map and input profile.
- `assets/scenes/maze.json` has the matching board geometry, camera, knight, Warden, seals and gate.
- Expand **Wall Torches** in the Editor hierarchy and select **Flame** or **Embers** to tune each particle emitter.
- `asset-reuse.json` records the original actor and masonry source hashes. The copied GLBs live in this project's own
  asset tree. Runtime and export use the maze's catalog.
- `assets/audio/beneath-the-stones.wav` is the original looping score.

Use `python tools/build_project.py --output "D:\Game Builds\Hollow Maze Copy"` to generate a fresh copy from the pinned
Cinder Oath assets. The destination directory must be new. Edit the checked-in scene directly for normal authoring. If
you change the tile map, keep the matching walls, jump barriers, starting actors, seals and gate in sync.
