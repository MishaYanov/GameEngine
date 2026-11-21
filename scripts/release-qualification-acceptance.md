# Courtyard release acceptance

## Build the candidate

Use Python 3.11 or later. On the Windows builder, start from a clean checkout with the locked Cargo dependencies and
Vulkan available. Run from the repository root with a fresh evidence directory.

~~~powershell
python -B scripts/qualify_release.py --require-clean apps/game/project.release-learning.json target/release-qualification/build-123
~~~

The manual Jenkins builder uses `ci/jenkins/Jenkinsfile.windows-release` on an `engine-windows-vulkan` worker in an
interactive native session. It archives `candidate.zip` and `qualification.json` after the copied Player runs with both
input configurations. Keep the pair from the same trusted build. Local builds that report uncommitted changes can be
rehearsed and stay pending under the release gate.

## Probe the delivered package on a separate Windows host

Use a disposable Windows account or a clean VM snapshot with the supported graphics driver. Transfer the verified build
artifact pair and trusted probe scripts. The probe needs Python, the Windows GPU inventory service, and Vulkan. Its
harness consists of `probe_installed_candidate.py`, `player_smoke.py`, `rehearse_release.py`, `verify_candidate.py`, and
`verify_host_gates.py` in one directory.

~~~powershell
$download = 'D:\ReleaseEvidence\build-123'
$zip = Join-Path $download 'candidate.zip'
$receipt = Join-Path $download 'qualification.json'
$probe = Join-Path $download 'clean-probe'
python -B scripts/probe_installed_candidate.py $receipt $zip $probe --host-profile clean-windows-vulkan
$probeStatus = $LASTEXITCODE
if ($probeStatus -ne 0) { Write-Warning 'Inspect the probe log and host metadata' }
~~~

The probe verifies the archive and each member, starts `delivered-player/game.exe` from a separate working directory
with disposable local application data, and rechecks the package after shutdown. Preserve `clean-player.gate.json`,
`clean-player.log`, and `windows-host.json` together. Use the host's assigned profile. A probe on the build workstation
is a local rehearsal.

## Collect independent gate results

A trusted release operator assigns four host profiles. Keep the clean Player host separate from the workspace and editor
workers. Run source checks at the exact receipt revision.

| Gate                | Observation to record                                                                                                                                                                                     |
|---------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `headless`          | Run the Courtyard route replay and semantic controller checks from the candidate's verified project against the matching headless build. Retain the report and command log.                               |
| `editor`            | Run native editor acceptance from the same clean source revision on the assigned Windows Vulkan editor worker. Retain the workspace test results and the in-window Game view and transition observations. |
| `clean_player`      | Collect the three files from the independent probe.                                                                                                                                                       |
| `manual_controller` | Run the physical test below on its assigned Windows host. Retain its row-by-row observations.                                                                                                             |

Use one result per gate. The independent probe writes its own result. For the remaining gates, place the trusted logs in
the evidence directory and use `record` with the relative names. A rerun uses a new evidence directory so the older
result remains available. Write logs in UTF-8 and keep diagnostic files under 64 MiB each.

~~~powershell
$evidence = Join-Path $download 'evidence'
$profiles = 'D:\ReleaseEvidence\host-profiles.json'
python -B scripts/verify_host_gates.py record $receipt $zip $evidence --gate headless --host-profile workspace-replay --status passed --evidence headless-report.json --evidence headless.log
python -B scripts/verify_host_gates.py verify $receipt $zip $evidence --profiles $profiles
$verdictStatus = $LASTEXITCODE
~~~

Exit 0 records accepted, exit 1 rejected, exit 2 pending, and exit 3 invalid evidence. Failed gates take precedence over
pending ones. Missing gates, `not_run` results, and dirty-source receipts keep acceptance pending when there are no
recorded failures. Hashes protect byte identity. Assign host profiles and authenticate build and tester origin through
the trusted release workflow.

For Jenkins, configure a separate manual Pipeline with `ci/jenkins/Jenkinsfile.windows-host-gates` and the
`engine-windows-release-coordinator` worker. Provision `ENGINE_WINDOWS_GATE_INBOX` as an administrator-managed inbox and
`ENGINE_WINDOWS_GATE_PROFILES` as a trusted profile-map file outside that inbox. Stage a new ordinary directory under
the inbox containing `candidate.zip`, `qualification.json`, and an `evidence` folder using the job's `BUNDLE_ID`. Only
trusted operators should publish bundles there. Keep successful and failed diagnostic logs together. The job archives
its decision and verified evidence, flags pending builds as unstable, and fails rejected or malformed submissions.

## Record the physical controller and saves

Run the delivered Player from an unrelated working directory and a disposable `LOCALAPPDATA`. Record the archive digest,
Git revision, Windows build, GPU, driver, controller model, connection type, tester, and observed result for every row.
Use physical hardware for controller tests.

| Check                                                                                                                                                           | Observation | Result |
|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------|--------|
| Keyboard opens Controls, navigates, applies and persists a movement preset                                                                                      |             |        |
| Menu, D-pad, A and B open Controls, remap Interact, and apply with visible focus                                                                                |             |        |
| The remap persists after relaunch and the small window keeps the focused row visible                                                                            |             |        |
| Closing Controls with a remapped B cannot interact until B is released and pressed again                                                                        |             |        |
| Focus loss and unplug clear ownership. Holding a button through reconnect needs release and a fresh press                                                       |             |        |
| Escape cancels Replace after disconnect. Held A on reconnect does not confirm                                                                                   |             |        |
| Save to Slots 1 and 2. Relaunch and Continue both checkpoints independently                                                                                     |             |        |
| Cancelling Replace leaves the slot bytes unchanged. Explicit Replace creates the prior generation                                                               |             |        |
| With the Player closed, corrupt only the disposable Slot 1 primary. Continue rejects it, Recover restores the prior generation, and Slot 2 bytes stay unchanged |             |        |
| An approved prior release slot loads without changing its bytes until Save. A separate incompatible save is rejected and preserved                              |             |        |

Record the physical gate as `passed` after all rows are observed, `failed` if a required row fails, or `not_run` when
the assigned hardware is unavailable. Keep the notes in `manual-controller.txt` within the evidence directory. Save
hashes and concise observations are enough for the handoff.

~~~powershell
python -B scripts/verify_host_gates.py record $receipt $zip $evidence --gate manual_controller --host-profile clean-windows-vulkan --status passed --evidence manual-controller.txt
~~~

## Attach playtest evidence for gameplay differences

Run the recorded Player with `--record-playtrace <new-file>` and the authored keyboard profile. Replay against the
manifest and assets from the same verified extraction. The playtest collector checks trace and manifest identity, and
the candidate verifier checks the archived files against the receipt. Re-run headless to confirm the reported runtime
facts. Keep physical ownership, menus, and save-recovery checks in the manual notes.

## Local review packet and decision

Run the review commands with the same verified receipt, archive, gate directory and trusted host profile map. Inspect
the packet before recording a held, rejected or accepted decision. Acceptance requires all four verified passes and a
clean source receipt. A changed receipt, profile, gate record, evidence file, or attached playtest handoff requires a
new packet and decision.

~~~powershell
python -B scripts/review_candidate.py prepare $receipt $archive $gateDirectory $packet --profiles $profiles
python -B scripts/review_candidate.py decide $receipt $archive $gateDirectory $packet $decision --profiles $profiles --scope windows-player --status held --reviewer 'QA operator' --reason 'Independent hardware check is pending'
python -B scripts/review_candidate.py verify-decision $receipt $archive $gateDirectory $packet $decision --profiles $profiles
~~~

The reviewer field records operator-entered text. Attach a previously verified reproduction with --playtest on all three
commands when it belongs to the same candidate. The attachment leaves the gate verdict unchanged.

## Relevant information and resources

- [Native two-process rehearsal](release-rehearsal-checklist.md) · [Approved predecessor evidence](../apps/game/fixtures/courtyard-release-learning/save-upgrade-evidence.md)
- [Candidate verifier](verify_candidate.py) · [Installed Player probe](probe_installed_candidate.py) · [Gate verifier](verify_host_gates.py)
- [Windows builder](../ci/jenkins/Jenkinsfile.windows-release) · [Host evidence job](../ci/jenkins/Jenkinsfile.windows-host-gates) · [Jenkins artifact archives and fingerprints](https://www.jenkins.io/doc/pipeline/steps/core/#archiveartifacts-archive-the-artifacts)
