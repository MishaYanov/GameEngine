# Courtyard release acceptance

Use Python 3.11 or later. Run the command below from the repository root on Windows with the locked Cargo dependencies
and native Vulkan available. Choose a fresh candidate directory for each attempt.

~~~powershell
python -B scripts/qualify_release.py apps/game/project.release-learning.json target/release-qualification/local-candidate-1
~~~

For CI, configure a manual Pipeline job from the trusted repository with script path
`ci/jenkins/Jenkinsfile.windows-release`. Assign an `engine-windows-vulkan` worker running an interactive native session
on a disposable account. A successful build provides a ZIP and receipt. Each attempt retains selected logs for
diagnosis.

## Check the delivered package

Download `qualification.json` and `candidate.zip` from the same trusted build into an empty directory. Run the
verification from the checkout against the exact download. Then extract to a fresh destination and run `game.exe` with
an unrelated working directory.

~~~powershell
$download = 'D:\ReleaseEvidence\build-123'
$zip = Join-Path $download 'candidate.zip'
$receipt = Join-Path $download 'qualification.json'
$package = Join-Path $download 'player'
python -B scripts/verify_candidate.py $receipt $zip
if ($LASTEXITCODE -ne 0) { throw 'Candidate verification failed' }
if (Test-Path -LiteralPath $package) { throw 'Choose a fresh package folder' }
Expand-Archive -LiteralPath $zip -DestinationPath $package
~~~

## Record what you observed

Copy this section into the build evidence directory. Use the same extracted candidate for every check and a disposable
Windows account and save profile. Record the machine and controller model for native input failures. A passed smoke test
confirms a Player frame from the copied package.

- Jenkins build or local run
- Git revision and dirty status
- Candidate ZIP SHA-256
- Tester, Windows build, GPU and driver
- Controller and connection type

| Check                                                                                              | Observation | Result |
|----------------------------------------------------------------------------------------------------|-------------|--------|
| Download receipt and all package files verify                                                      |             |        |
| Keyboard opens Controls, navigates, applies and persists a preset                                  |             |        |
| Controller loses ownership on focus loss and unplug. Held buttons require release after reconnect. |             |        |
| Fresh route reaches the gate and saves the `after_gate` checkpoint                                 |             |        |
| Next process continues, finishes, then New Run resets route progress                               |             |        |
| Approved prior release `after_gate` save continues and remains byte-identical until Save           |             |        |
| A changed or unapproved save is rejected and retained                                              |             |        |

**Decision**  
Accepted, pending, or rejected. Give a reason and reproduction steps for each failed or blocked check. Keep raw save
files and profile contents out of the candidate artifacts.

## Playtest evidence

Verify the candidate ZIP with its trusted local receipt before extracting it into a fresh directory. Record from that
Player with `--record-playtrace <new-file>` and the authored keyboard profile. Replay the trace against the manifest and
assets from that same extracted copy.

The playtest collector checks the trace and manifest digests reported by headless. The separate candidate verifier
checks every archived file against the receipt. The handoff records those inputs for reproduction. To verify runtime
facts independently, replay the trace against the verified candidate and compare the generated report.

## Relevant information and resources

- [Native two-process rehearsal](release-rehearsal-checklist.md)
- [Windows CI archive and fingerprint behavior](https://www.jenkins.io/doc/pipeline/steps/core/#archiveartifacts-archive-the-artifacts)
- [ZIP reader and archive modes](https://docs.python.org/3.11/library/zipfile.html#zipfile.ZipFile)
