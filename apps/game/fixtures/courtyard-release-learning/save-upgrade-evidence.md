# Courtyard checkpoint upgrade evidence

The reviewed change renames the unbound decoration with entity key `courtyard-cp-entry` from `Entry checkpoint marker`
to `Entry checkpoint floor marker`. Keep the [release manifest](../../project.release-learning.json) and every other
field of the [Courtyard scene](../../assets/scenes/courtyard-release-learning.json) identical. This retains the
`courtyard/after_gate` ID, spawn and radius, key and gate requirements, and route rules.

| Identity                             | Value                                                              |
|--------------------------------------|--------------------------------------------------------------------|
| Project save ID                      | `c70545a2-181c-42d0-906e-87fb32547914`                             |
| Slot record version                  | `1`                                                                |
| Prior project and scene fingerprint  | `9e544326348ac29e0ec201de9e237967b15448cd46d6493a961ac15eb122603e` |
| Target project and scene fingerprint | `599c5ac355f83c7c949b3901e27b57f9367ae03ca47a7669a60128ed8f560e41` |

These values come from the checkpoint store’s normalized Rust project and scene serialization. The prior value matches
both the original authored fixture and the verified Milestone 15 copied package. Keep the same manifest for the updated
export. The [reviewed rule](../../../../crates/courtyard-gameplay/src/checkpoint_compatibility.rs) approves only this
direction and checkpoint.
The [synthetic old record](../../../../crates/courtyard-gameplay/tests/fixtures/courtyard-prior-save.json) contains only
fixture data.

## Verify a copied upgrade

Export and verify both packages with [the release rehearsal script](../../../../scripts/rehearse_release.py). If the
prior export is unavailable, prepare an isolated copy of the current authored manifest and assets and restore only the
decoration’s old label before exporting that fixture. The native test uses fresh Windows Vulkan processes and one
disposable profile.

~~~powershell
$env:GAME_ENGINE_QA_PRIOR_PROJECT = 'C:\path\to\copied-prior\project.json'
$env:GAME_ENGINE_QA_PROJECT = 'C:\path\to\copied-current\project.json'
cargo test -p courtyard-gameplay --lib --offline --locked checkpoint_compatibility
cargo test -p game --bin game --offline --locked player_controls
cargo test -p game --bin game --offline --locked save_package_upgrade -- --ignored --nocapture
~~~

Set `GAME_ENGINE_QA_UNAPPROVED_PROJECT` to a separately staged third manifest if available. The rejection phase
otherwise changes the target project name in memory so the candidate has an unapproved fingerprint.

| Package    | Action                         | Expected result                                                                                              |
|------------|--------------------------------|--------------------------------------------------------------------------------------------------------------|
| Prior      | Save at `after_gate` and exit  | The slot contains the prior fingerprint and the committed key and gate facts.                                |
| Unapproved | Try Continue from a fresh run  | Controls explain the incompatibility. The active run and each slot byte remain intact.                       |
| Updated    | Continue and exit without Save | The Player restores the key and open gate at `after_gate`. The slot retains its prior bytes.                 |
| Updated    | Continue and explicitly Save   | The slot contains the target fingerprint. A fresh process identifies it as current and can finish the route. |

The native test drives the Player’s action methods and renders each hidden window. For a manual rehearsal, follow the
same rows using Controls and record the package hash, visible status, and slot hash along with the
result. [Milestone 17.3](../../../../.learning-guides/milestone-17-save-compatibility/17.3-package-upgrade-rehearsal.feature.md)
