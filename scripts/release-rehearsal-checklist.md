# Cinder Oath Windows release rehearsal

Record the source revision and working-tree changes, Windows/GPU/Vulkan version, manifest location, package SHA-256
report, and the exact copied executable and independent working directory. Use one clean test profile for both launches.
Start each visible launch with the same two-scene package. Pass a fresh, ordinary `--transfer-directory`
for the practice checkpoint and audit exercise. Use a disposable `LOCALAPPDATA` for both launches.

From the repository root, export and validate the package first.

~~~powershell
python -B scripts/rehearse_release.py projects/cinder-oath/project.json target/release-rehearsal
~~~

Copy the exported package to a new path inside its evidence directory. Verify the original and the copy against the
local export report before opening either executable. Keep the copied package and test profile for the sequence below.

~~~powershell
$run = Read-Host 'Paste the path printed after Evidence directory'
$reportPath = Join-Path $run 'export-report.json'
$report = Get-Content -Raw -LiteralPath $reportPath | ConvertFrom-Json
$copy = Join-Path $run 'copied-player'
Copy-Item -LiteralPath $report.export_directory -Destination $copy -Recurse
python -B scripts/rehearse_release.py --verify-copy $reportPath $copy
$testProfile = Join-Path $run 'test-profile'
$transfer = Join-Path $run 'transfers'
New-Item -ItemType Directory -Path $testProfile, $transfer | Out-Null
~~~

Use the same paths for both runs. The copied Player starts from the separate evidence directory.

~~~powershell
$previousProfile = $env:LOCALAPPDATA
$env:LOCALAPPDATA = $testProfile
Push-Location -LiteralPath $run
try {
    & (Join-Path $copy 'game.exe') --transfer-directory $transfer
} finally {
    Pop-Location
    $env:LOCALAPPDATA = $previousProfile
}
~~~

| Run   | Observable check                                                                                        | Observed | Result |
|-------|---------------------------------------------------------------------------------------------------------|----------|--------|
| One   | Esc opens Controls. Project defaults are active.                                                        |          |        |
| One   | Change the layout to Arrow keys with Left/Right. Move to Apply with Down and press Enter.               |          |        |
| One   | Watch the knight introduction. Enter the arena and confirm movement plus the configured Attack action.  |          |        |
| One   | Choose Controls → Cinder Oath Checkpoints → Save Arena Entry. Quit the process.                         |          |        |
| Two   | Esc opens Controls with Arrow keys active. Choose Continue Practice Slot under Cinder Oath Checkpoints. |          |        |
| Two   | The duelist and the Cinder Warden are both ready at the start of the fight.                             |          |        |
| Two   | Defeat the Cinder Warden and observe victory. F10 begins again with the knight introduction.            |          |        |
| Two   | Controls stay on Arrow keys and the checkpoint remains available. Close the process.                    |          |        |
| Final | Verified copy still matches the package inventory.                                                      |          |        |

For each failed or skipped check, record what happened and link its log. A hidden Vulkan frame confirms native startup.
The observations above exercise actual input, presentation, and persistence.

## Relevant information and resources

- [Release candidate and independent gates](release-qualification-acceptance.md)
- [Cinder Oath project manifest](../projects/cinder-oath/project.json)
- [Practice checkpoint transfer guide](../.learning-guides/milestone-67-checkpoint-transfer-preflight/README.md)
