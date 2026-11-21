# Courtyard Release Learning route verification

Use `apps/game/project.release-learning.json`. The route starts in `courtyard` and completes in `finish`. The standard
demo uses its own manifest and scene files.

## Run the headless route

From the repository root, run the input fixture and the integration target. The replay submits complete held physical
key sets under schema version 3. It pairs the E press with an immediate release before the following tick.

~~~powershell
cargo run -p engine-headless --offline --locked -- --project apps/game/project.release-learning.json --replay apps/game/fixtures/courtyard-release-learning/route.replay.json
cargo test -p engine-headless --test cli --offline --locked release_learning_route
~~~

The expected report has `active_scene_id: "courtyard"`, `play_scene_id: "finish"`, and `play_status.gameplay` with
`score: 1` and `completed: true`. The integration check also samples the authored gate between replay phases.

## Walk the exported route

Use the movement keys shown in Controls. Project defaults use A/D on the route’s X axis, and the Arrows preset uses
Left/Right. E remains the authored Interact key. Time the visible action against the landmarks rather than copying
fixed-step counts from the replay.

| Step | Action                                                                                                                   | Expected                                                                                                                                                |
|------|--------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------|
| 1    | Launch the copied Player and start New Run.                                                                              | Spawn near the green entry marker at X = 0. The route gate is closed and the key is visible to the right.                                               |
| 2    | Move right into the yellow key, then left toward the blue lever.                                                         | The key disappears and score is 1. The blue lever is at X = 0.15, Z = 0.22 and is reachable from the center route inside the 0.42 interaction distance. |
| 3    | Release movement and tap E once.                                                                                         | The committed interaction opens the purple route gate at X = −0.70.                                                                                     |
| 4    | Move left past the gate and stop at the green marker near X = −0.97. Press Esc, select Save checkpoint, and press Enter. | The Player shows Saved after_gate. The single manual slot records the key and gate facts. The checkpoint radius is 0.12 at `[-0.97, 0, 0]`.             |
| 5    | Close the process, reopen the same package, and choose Continue in Controls.                                             | The player returns to the after-gate marker with the gate open, the lever consumed and score 1.                                                         |
| 6    | Move left through the original red exit, then right toward the finish marker.                                            | The scene changes to `finish`. Reaching its marker completes the run.                                                                                   |
| 7    | Press F10 after completion.                                                                                              | New Run returns to the entry marker. The key and closed gate return with fresh progress. The manual slot remains available.                             |

## Record after verification

- Source revision and changed file list.
- Headless report and the fixed-step state checks.
- Export path and package digest, Windows/GPU/driver, and effective input preset.
- Visible startup, Continue, finish, and New Run results for the same copied executable.
- Missing or failed check, its concrete cause, and the evidence path.
