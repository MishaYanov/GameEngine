# Cinder Oath release acceptance

## Build the candidate

Use Python 3.11 or later. On the Windows builder, start from a clean checkout with the locked Cargo dependencies and
Vulkan available. Run from the repository root with a fresh evidence directory.

~~~powershell
python -B scripts/qualify_release.py --require-clean projects/cinder-oath/project.json target/release-qualification/build-123
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
| `headless`          | Replay the Cinder Oath intro and duel against the verified manifest with the matching headless build. Retain the report and command log.                                                                  |
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

| Check                                                                                                                | Observation | Result |
|----------------------------------------------------------------------------------------------------------------------|-------------|--------|
| Keyboard opens Controls, navigates, applies and persists a movement preset                                           |             |        |
| Menu, D-pad, A and B open Controls, remap Attack, and apply with visible focus                                       |             |        |
| The remap persists after relaunch and the small window keeps the focused row visible                                 |             |        |
| Closing Controls with a remapped B requires a fresh press before the next attack                                     |             |        |
| Focus loss and unplug clear ownership. Holding a button through reconnect needs release and a fresh press            |             |        |
| Escape cancels a pending import after disconnect. Reconnect with A held, then release it and press again to confirm  |             |        |
| Reach the arena and save its practice entry. Relaunch and resume the practice slot at the start of the duel          |             |        |
| Export the practice checkpoint and preview it in a second disposable profile. Cancel once, then review and import it |             |        |
| Inspect the exported audit. Record the transfer scope, outcome and UTC completion time                               |             |        |

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

## Detached signer verification

The release operator selects a local OpenSSH key and the verification operator supplies the trusted allowed-signers
policy and principal. Signing reconstructs the current review before requesting a signature. Verification checks the
captured decision against the signer policy first, then reconstructs the review. Both commands use the system OpenSSH
executable by default. Use --ssh-keygen with the absolute path of a trusted installation when OpenSSH is installed
outside the system directory. Pass the same optional playtest attachment to both commands.

~~~powershell
python -B scripts/sign_release_review.py $receipt $archive $gateDirectory $packet $decision $signature --profiles $profiles --key $localSigningKey
python -B scripts/verify_release_review_signature.py $receipt $archive $gateDirectory $packet $decision $signature --profiles $profiles --allowed-signers $allowedSigners --principal $principal
~~~

A valid signature retains the review decision's existing held, rejected or accepted status. Independent gate evidence
still determines whether acceptance is available.

## Combine release and QA signoffs

Write a trusted role policy outside the candidate and evidence folders. Give each required role one to eight permitted
principals. Use independent signing keys, even when each role names a different principal. Pick the maximum age for both
decisions in seconds, up to 31 days.

~~~json
{
  "schema_version": 1,
  "max_decision_age_seconds": 86400,
  "roles": {"release": ["release-owner"], "qa": ["qa-owner"]}
}
~~~

Each reviewer signs a decision for the same current packet using the detached signing command. Pass each role,
principal, decision and signature together. Use the same independently provisioned allowed-signers and optional
revocation files for both roles. Pass the original optional playtest folder when the packet includes it.

~~~powershell
$inputs = @($receipt, $archive, $gateDirectory, $packet, $ledger,
    '--profiles', $profiles, '--role-policy', $rolePolicy, '--allowed-signers', $allowedSigners,
    '--signoff', 'release', $releasePrincipal, $releaseDecision, $releaseSignature,
    '--signoff', 'qa', $qaPrincipal, $qaDecision, $qaSignature)
python -B scripts/verify_release_signoffs.py publish @inputs
$ledgerStatus = $LASTEXITCODE
python -B scripts/verify_release_signoffs.py verify @inputs
$verificationStatus = $LASTEXITCODE
~~~

Publish uses a fresh ledger path. Verify rechecks every supplied signature, rebuilds the current packet and compares the
complete stored ledger. Exit 0 means accepted, 1 means rejected, 2 means held, and 3 means invalid evidence or output. A
missing or expired role keeps a pending decision held. A current rejection or a rejected gate takes precedence.
Acceptance requires both current approvals and all of the existing clean-build and independent-host gates.

## Publish or verify a movable release handoff

Copy `qualification.json`, `candidate.zip`, `packet.json` and `ledger.json` into an ordinary handoff root. Copy gate
records and the files they reference into `evidence/`, keeping the relative paths from each gate record. For every role
in the ledger with evidence, add its exact files as `signoffs/release-decision.json`, `signoffs/release-decision.sig`,
`signoffs/qa-decision.json` or `signoffs/qa-decision.sig`. If the packet binds an attached replay, copy the three
verified playtest files into `playtest/`.

Supply the host profiles, role policy, allowed signers and optional revocations independently of the handoff. Keep each
of those files and all private signing keys outside the handoff directory. When the policy has a revocation list, pass
its path as `--revoked-signers` to both commands. Publish creates a fresh `handoff.json` with the relative names, sizes
and SHA-256 hashes of up to 64 retained artifacts. Verification repeats the current ledger checks and compares the
complete index.

~~~powershell
$policy = @('--profiles', $profiles, '--role-policy', $rolePolicy, '--allowed-signers', $allowedSigners)
python -B scripts/release_signoff_handoff.py publish $handoff @policy
$publishStatus = $LASTEXITCODE
python -B scripts/release_signoff_handoff.py verify $relocatedHandoff @policy
$verificationStatus = $LASTEXITCODE
~~~

At the destination, preserve the relative paths listed in `handoff.json` and copy that index alongside them. Create an
empty `evidence/` directory when the review contains no gate records. Exit 0 means accepted, 1 rejected, 2 held, and 3
invalid or changed evidence. A current rejection still takes precedence over a missing role. If a decision expires after
publication, reconstruct the current ledger and publish a fresh handoff from reviewed evidence.

## Compare current handoffs

Supply both roots under the independently provisioned policy. The comparison reconstructs both indexes and rechecks
each ledger at the same UTC instant. It writes a new JSON file outside both roots with candidate and packet SHA-256
values, both verdicts, and sorted lists of added, removed, and changed files. Each file includes its relative path,
byte count, and SHA-256. A comparison exit of 0 means both inputs verified and the report was written. Read the
verdicts inside the report before making a release decision. Exit 3 means the report could not be written or an
input failed verification, including expiration.

~~~powershell
python -B scripts/release_signoff_handoff.py compare $priorHandoff $currentHandoff $comparison @policy
$comparisonStatus = $LASTEXITCODE
~~~

## Renew release and QA decisions

1. Retain the original handoff for the audit. Create a fresh ordinary directory with exact copies of the candidate
   and its qualification receipt. Collect current gate records and their referenced evidence in its `evidence/`
   directory. Use the same trusted host profiles, signer policy, and role policy for every verification command.
2. Prepare a fresh packet and review the gate results. Use the same optional `--playtest` argument throughout when
   attaching verified playback evidence. Each reviewer records a current decision for that packet and signs it
   with their independently provisioned key. Set `$releaseStatus` and `$qaStatus` to their actual decisions.
3. Publish a fresh ledger using the decision and signature pairs available now. Provide one `--signoff` group for
   each completed role. Release acceptance requires both current approvals and all qualifying gate results. A
   missing approval leaves an otherwise passing review held.
4. Publish and verify the new handoff. Check the verdict again from the relocated directory if someone will receive
   the evidence there. If current evidence changes or another decision expires, repeat renewal in another directory.

~~~powershell
$receipt = Join-Path $renewedRoot 'qualification.json'
$archive = Join-Path $renewedRoot 'candidate.zip'
$gates = Join-Path $renewedRoot 'evidence'
$packet = Join-Path $renewedRoot 'packet.json'
$ledger = Join-Path $renewedRoot 'ledger.json'
$signoffs = New-Item -ItemType Directory -Path (Join-Path $renewedRoot 'signoffs')
$releaseDecision = Join-Path $signoffs.FullName 'release-decision.json'
$releaseSignature = Join-Path $signoffs.FullName 'release-decision.sig'
$qaDecision = Join-Path $signoffs.FullName 'qa-decision.json'
$qaSignature = Join-Path $signoffs.FullName 'qa-decision.sig'
$reviewInputs = @($receipt, $archive, $gates, $packet)
python -B scripts/review_candidate.py prepare @reviewInputs --profiles $profiles
python -B scripts/review_candidate.py decide @reviewInputs $releaseDecision --profiles $profiles --scope windows-player --status $releaseStatus --reviewer $releaseReviewer --reason $releaseReason
python -B scripts/sign_release_review.py @reviewInputs $releaseDecision $releaseSignature --profiles $profiles --key $releaseKey
python -B scripts/review_candidate.py decide @reviewInputs $qaDecision --profiles $profiles --scope windows-player --status $qaStatus --reviewer $qaReviewer --reason $qaReason
python -B scripts/sign_release_review.py @reviewInputs $qaDecision $qaSignature --profiles $profiles --key $qaKey
$roleInputs = @('--signoff', 'release', $releasePrincipal, $releaseDecision, $releaseSignature,
    '--signoff', 'qa', $qaPrincipal, $qaDecision, $qaSignature)
python -B scripts/verify_release_signoffs.py publish @reviewInputs $ledger @policy @roleInputs
$ledgerStatus = $LASTEXITCODE
python -B scripts/release_signoff_handoff.py publish $renewedRoot @policy
$publishStatus = $LASTEXITCODE
python -B scripts/release_signoff_handoff.py verify $renewedRoot @policy
$verifyStatus = $LASTEXITCODE
~~~

## Generate a renewal checklist

Use the receipt, archive, gate directory, packet and any signed role decisions explicitly selected from the previous
review. Supply trusted profiles, signer policy and verification tools from outside the evidence and playtest folders.
Record any rejected role or failed gate
first, then collect decisions for roles listed in `roles_requiring_review`. A dirty source is recorded in
`source_dirty`.
The checklist is a local diagnostic file. An expired publication must go through the renewal steps above.

~~~powershell
$reviewInputs = @($receipt, $archive, $gates, $packet)
python -B scripts/verify_release_signoffs.py checklist @reviewInputs $checklist @policy @roleInputs
$checklistStatus = $LASTEXITCODE
~~~

Write the checklist to a fresh path outside the gate and playtest evidence folders. A successful write returns the
verified verdict exit code. A held review with an expired release decision and missing QA decision lists both roles.
A rejected review preserves the rejection even when the other role is missing.

## Summarize a current verified handoff

~~~powershell
python -B scripts/release_signoff_handoff.py summarize $relocatedHandoff $summary @policy
$summaryStatus = $LASTEXITCODE
~~~

Write the summary outside the handoff in an existing ordinary directory. Schema 1 includes the archive, packet and
receipt SHA-256 hashes, source revision and dirty flag, role states, gate verdict and overall status. Each gate lists
its
trusted profile, result-record SHA-256, and any case files as path, byte size and SHA-256. Resolve each case path from
the
verified handoff root. An absent gate is `pending` and has an empty evidence list.

~~~powershell
$report = Get-Content -Raw -LiteralPath $summary | ConvertFrom-Json
foreach ($gate in $report.gates) {
    foreach ($case in $gate.evidence) {
        $actual = (Get-FileHash -LiteralPath (Join-Path $relocatedHandoff $case.path)).Hash.ToLowerInvariant()
        if ($actual -ne $case.sha256) { throw "Case changed at $($case.path)" }
    }
}
~~~

Exit 0 means accepted, 1 rejected, 2 held, and 3 invalid. A dirty candidate with missing gates yields a held summary.
An active rejection yields a rejected summary even when another role has yet to sign. A changed file or an index that
no longer matches current role freshness prevents summary creation. Keep the report local to the operator.

## Rehearse the renewal locally

Run the disposable OpenSSH fixture suite from the repository root. It recreates an archived approval, renews one role
and then both roles, verifies the relocated result, and confirms that the expired comparison returns 3 and leaves the
prior handoff intact. The fixtures provision their own profiles, keys and policy.

~~~powershell
$env:PYTHONPATH = 'scripts'
python -B -m unittest discover -s tests -p test_release_signoff_handoff.py
$fixtureExit = $LASTEXITCODE
~~~

For a local Windows rehearsal, create a disposable working directory and qualify Cinder Oath without
`--require-clean`. Build a packet for the actual receipt using a local host profile map. Use disposable signers or
leave roles explicitly missing. Write rehearsal notes alongside the outputs with the receipt and packet SHA-256
values, verified gate statuses, command exit codes, verdict, and policy origin. Keep key paths and credentials out of
the notes. Repeat using approved operator policy and independent workers when collecting real release evidence.

## Relevant information and resources

- [Native two-process rehearsal](release-rehearsal-checklist.md) · [Cinder Oath project manifest](../projects/cinder-oath/project.json)
- [Candidate verifier](verify_candidate.py) · [Installed Player probe](probe_installed_candidate.py) · [Gate verifier](verify_host_gates.py)
- [Windows builder](../ci/jenkins/Jenkinsfile.windows-release) · [Host evidence job](../ci/jenkins/Jenkinsfile.windows-host-gates) · [Jenkins artifact archives and fingerprints](https://www.jenkins.io/doc/pipeline/steps/core/#archiveartifacts-archive-the-artifacts)
