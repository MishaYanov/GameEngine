# Courtyard Windows release rehearsal

Record the source revision and working-tree changes, Windows/GPU/Vulkan version, manifest location, package SHA-256
report, and the exact copied executable and independent working directory. Use one clean test profile for both launches.
Start each visible launch with the same two-scene package and no arguments.

From the repository root, export and validate the package first.

~~~powershell
python scripts/rehearse_release.py apps/game/project.release-learning.json target/release-rehearsal
~~~

Copy the exported package to a new path inside its evidence directory and use the installed smoke test
from [Milestone 15.2](../.learning-guides/milestone-15-release-rehearsal/15.2-installed-player-smoke.feature.md). Verify
the original and the copy against the local export report before opening either executable. Keep the copied package and
test profile for the sequence below.

~~~powershell
python scripts/rehearse_release.py --verify-copy "$run/export-report.json" "$run/copied-player"
~~~

| Run   | Observable check                                                                                          | Observed | Result |
|-------|-----------------------------------------------------------------------------------------------------------|----------|--------|
| One   | Esc opens Controls. Project defaults are active.                                                          |          |        |
| One   | Change the layout to Arrow keys with Left/Right. Move to Apply with Down and press Enter.                 |          |        |
| One   | Move right to the key, back left to the blue lever, and use the displayed Interact key. Gate opens once.  |          |        |
| One   | Walk beyond the gate to the checkpoint and choose Save checkpoint in Controls. Quit the process.          |          |        |
| Two   | Esc opens Controls with Arrow keys active. Choose Continue and return to play.                            |          |        |
| Two   | Player resumes beyond the gate with the key collected and the gate open. The opening cue does not replay. |          |        |
| Two   | Reach the existing exit and finish marker. F10 starts at Courtyard with fresh progress.                   |          |        |
| Two   | Controls stay on Arrow keys and the checkpoint remains available. Close the process.                      |          |        |
| Final | Verified copy still matches the package inventory.                                                        |          |        |

For each failed or skipped check, record what happened and link its log. A hidden Vulkan frame confirms native startup.
The observations above exercise actual input, presentation, and persistence.
