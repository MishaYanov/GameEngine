# Original game-building milestones

This is the completed Milestone 1–3 review. The later [Milestone 4–6 implementation](milestones-4-6-review.md)
adds scene4 components, linked source/override workflows and runtime entity lifecycle inspection.

This ledger records the recovered sixteen-feature plan. The baseline is commit `1c24b9f`, inspected with the
working tree on 2026-11-20. Source state and test evidence are recorded separately so an implemented API or
mocked frontend check does not imply native acceptance.
The final verification summary is `history/verification/milestones-tests-2026-11-20.json`; the inspected source
fingerprints and verification limits are recorded in `docs-site/docs-state.json`.

## Milestone 1

| Feature | Current source behavior | Verification scope |
| --- | --- | --- |
| 1.1 Gameplay module metadata | The module selector and Gameplay Roles use native descriptors. Unavailable selections remain repairable | Existing metadata, history and compatibility checks plus the new configured Finish-role regression |
| 1.2 Collider Inspector | None, Box and Sphere, offsets and sizes use the shared edit coordinator | Numeric drafts, rollback, history, stale-context and persistence regressions |
| 1.3 Sensors and collision filtering | Blocking/Sensor mode and 32 layer and mask controls, including All and None | Unsigned highest-bit, zero groups, symmetric filtering, bulk Undo and read-only tests |
| 1.4 Trigger Enter/Exit events | Directed deterministic sensor edges reach gameplay and tick consumers | Bootstrap, step partitioning, pause, scene replacement, rollback and work-budget tests in `trigger_runtime_api.rs` |
| 1.5 Authored interaction settings | The current scene can select Course or Finish sensors, a destination and an optional exit blocker | Frontend typed payload, validation and context tests. The native factory validates references and collision compatibility |

Trigger sampling runs at fixed-step start. Movement and collision changes from one step become eligible for
sampling at the next step. A final movement keeps sampling awake after input release. A stationary sensor can
produce its initial overlap without movement. Pause retains tracker history, while restart and scene replacement
begin with a fresh tracker. Rejected ticks restore world properties, progress, camera and trigger history together.

The tracker allows at most 8,192 directed pairs, 16,384 events per tick and a 4 MiB key/event byte allowance.
Exit events sort before Enter events, with stable entity-key ordering within a phase. Current overlaps are
available through a read-only view. Consumers can query an overlap without requiring a repeating Stay event.

## Milestone 2

| Feature | Current source behavior | Verification scope |
| --- | --- | --- |
| 2.1 Kinematic slide | A bounded sweep projects remaining motion along contact surfaces and exposes up to three contact results | Collision and slide regressions. Curved shapes and hierarchy bounds remain conservative |
| 2.2 Follow camera | Optional scene camera follow settings share a CPU runtime rig in editor Play and Player | Core camera checks, frontend validation and command-queue Save Scene/reopen coverage |
| 2.3 Runtime visibility and collision | Typed actions change visibility and collision independently during Play | Runtime action validation and whole-tick rollback checks |
| 2.4 Gameplay feedback | Key pickup hides the key and disables its collision. The exit changes tint and its configured blocker unlocks | Shared Courtyard module tests and route replay |
| 2.5 Game HUD | Editor Game shows status beside the native aperture. Player prepares an in-window status overlay from the same Play status | Angular status tests and native adapter checks. Visible desktop acceptance remains independently recorded |

The runtime camera follows an entity's world position. Its authored eye and look offsets translate together.
Smoothing affects the rig position and uses seconds, with zero applying the target position immediately.
The rig keeps the orientation defined by its offsets during abrupt target reversals. Editor Scene navigation
continues to use its own camera. Stop returns to the authored scene camera, and restart or a new scene resolves
the new scene's camera.

## Milestone 3

| Feature | Current source behavior | Verification scope |
| --- | --- | --- |
| 3.1 Asset identity | UUID catalog entries and optional entity asset IDs retain legacy path compatibility | Identity/invalidation, repeatable migration, Unicode aliases, supported moves and real-file host reopen tests |
| 3.2 External GLB import | Import GLB chooses an external source and a contained project destination | CPU rejection checks cover unused images, invalid geometry and external dependencies; native checks prepare GPU candidates before publication |
| 3.3 Safe reimport | Reimport carries the captured asset ID and fingerprint and preserves the old asset on rejected preparation | CPU compatibility includes hidden entities, inactive scenes and uninstantiated prefabs. Hidden Vulkan checks cover failed budgets, candidate tampering, accepted cache promotion and retained old owners |
| 3.4 Gameplay prefabs | Save Gameplay Prefab and catalogued Instantiate Prefab controls use the existing guarded endpoints | Real-file host persistence and supported moves; actual Courtyard bindings, behavior and interaction-reference remapping, role-conflict rejection and atomic Undo/Redo checks |
| 3.5 Dependency export | Compiler preparation captures the dependency closure and validates gameplay before publication | Compiler tests cover source-independent staging, identity/dependency failures, URI containment and depth bounds. A fresh release export passes six headless checkpoints and a hidden native route |
| 3.6 Performance baseline | A repeatable representative scene measures CPU simulation and hidden native rendering | Five CPU runs and a hidden release Vulkan baseline passed. The [performance baseline](../../docs/performance-baseline.md) identifies inputs, resource counts, timing scopes and local budgets |

The individual feature checks distinguish frontend tests, real-file host operations and hidden native Vulkan
execution. The final workspace aggregate passed 793 tests across 70 test binaries, with 7 ignored and no
failures, including native DTO serialization checks. Frontend tests alone do not establish the native behavior
described in these rows. Final release-package evidence is recorded separately.

## Author and play the course

1. Open a copy of `apps/game/project.json` in the desktop editor. Select the Courtyard scene and inspect its
   Player, Key and Exit bindings. The hidden sensor entities are separate from the visible markers.
2. In Collider Inspector, check the Player collider and each trigger's Sensor mode. Their memberships and masks
   must accept each other. An empty membership or mask intentionally disables detection.
3. Open **Interaction Settings…**. Select **Course sensors**, choose distinct Key and Exit triggers, a different
   destination configured as Finish, and an optional blocking exit entity. On the destination, configure its Finish
   sensor and Player/Finish visual roles. **Legacy proximity** removes the active scene's override.
4. Open **Game Camera…**, choose Follow, select the player and edit world offsets and smoothing. Apply and Save All.
   Camera offsets must form a finite view within the supported pitch range.
5. Start Play and focus the native Game surface. Observe the objective and score while moving to the key.
   The key disappears and the exit unlocks. Enter the exit sensor, release the held movement key after the
   transition, and move to the Finish sensor.
6. Confirm completion stops gameplay movement. Restart Run returns to the retained startup scene with fresh
   progress. Stop returns to the authored scene, bindings, transforms and camera.

The input manager edits movement and action bindings in the project. Its **Test bindings** area needs focus
to receive keys. Apply changes, close the dialog and use Save All before reopening the project or exporting.
Focus Game again after returning from a WebView control. Escape releases Game keyboard ownership.

## Asset authoring workflow

**Import GLB…** chooses a self-contained binary glTF source and asks for a destination relative to Assets.
The destination folder must already exist. A successful operation adopts the returned snapshot and refreshes
the catalog. **Move Asset…** operates on asset identity for GLB models and saved JSON prefabs. The selected
catalog entry remains selected when its path changes. Legacy models with external resources remain readable;
import a self-contained GLB copy to use move or reimport.
The defaults use filenames directly inside Assets; choosing a subfolder requires that folder to exist.
Saving or moving an asset cannot claim a registered scene destination, including a scene whose draft has not
yet been saved to disk.

**Reimport GLB…** chooses a replacement source for the selected model. It carries the captured fingerprint so
a changed asset can be rejected for review. **Save Gameplay Prefab…** captures the reviewed selection after
pending Inspector edits finish. Saving to an existing prefab updates its assembly while retaining its identity.
The catalog exposes saved prefabs with **Instantiate Prefab**. A gameplay assembly carries its selected bindings,
behavior definitions and the module's declared interaction references. Include every referenced entity in the
selection; unresolved external entity references and conflicting destination roles are reported before placement.
Validation checks and remaps the native module's explicit typed entity and scene references before placement.
Implicit required roles, such as Player, can still need authoring and are validated when starting Play or
exporting. Saving and placing a valid partial assembly is supported incremental authoring work.

Asset operations retain their original workspace context. Replacing the project while a file chooser or
preparation is pending drops stale frontend results. Destination paths remain contained, and occupied import or
move destinations are preserved. CPU preparation and live GPU preparation precede publication. Rejected
preparation retains the old files and rendered resources. Publication rolls back an ordinary failed multi-file
operation; it is not a crash-atomic database.

Export roots include every registered scene and every saved prefab, including valid uncataloged legacy prefab
JSON anywhere inside Assets. The closure follows entity model IDs and
legacy path aliases, glTF buffer/image URIs and catalog dependencies. The exported subset catalog removes external
authoring source paths. Scene and prefab bytes are preserved. Preparation captures and hashes the same source
buffers, then publication rejects changed inputs. Dependency traversal rejects excessive depth, missing identities,
remote URIs and paths outside Assets. See `target/milestones-compiler-dependencies.log` for the compiler checks.

The editor suite passed 425 tests across 46 files, including accessibility checks, and the production bundle
check passed. The logs are `target/milestones-angular-tests.log` and `target/milestones-angular-production.log`.
The production check retains the existing project-setup stylesheet budget warning. These are frontend checks;
native asset publication, GPU resource replacement and exported dependency acceptance have separate evidence.

## Verification evidence

| Log | Observed result | Scope |
| --- | --- | --- |
| `target/milestones-rust-tests.log` | 793 passed, 7 ignored | Final workspace all-target/all-feature suite across 70 test binaries, including host, compiler and DTO contract tests |
| `target/milestones-rust-doc-tests.log` | Passed | Workspace Rust documentation checks; no doctest examples |
| `target/milestones-docs-site.log` | 60 pages, 7 tests passed | Static-site build, local link/anchor checks and empty Game App Developer space |
| `target/milestones-authored-interactions.log` | 6 passed | Actual Courtyard sensor route, strict settings, custom destination persistence and independent gameplay prefab reference remapping with Undo/Redo |
| `target/milestone2-native-course.log` | 1 passed | Hidden Vulkan Player drives the authored trigger/collider/follow-camera course, Finish, F10 restart and focus-loss neutral input; no Error log |
| `target/milestone3-native-reimport.log` | 1 passed | GPU-budget and candidate-tamper failures retain old files/catalog/resources. Accepted reimport promotes prepared resources; old owners remain accounted. Moving a model retains its GPU owner. Initially hidden model visibility and Stop restoration are checked |
| `target/milestone3-resource-cache-tests.log` | 4 passed | Focused resource-cache tests |
| `target/milestone3-gpu-host-tests.log` | 1 passed | Stale workspace/revision and expired host publication request are rejected without replacing files |
| `target/milestone3-identity-tests.log` | 4 passed | Stable identity, reserved aliases, malformed catalog rejection and editor invalidation/history behavior |
| `target/milestone3-project-assets-tests.log` | 7 passed | Exclusive publication, import validation, reimport rollback, abandoned/stale candidate cleanup, bounded JSON inventory, migration and prefab/model moves |
| `target/milestone3-settings-budget-tests.log` | 3 passed | Shared project/prefab settings depth, node and byte limits are checked before cloning or serialization |
| `target/milestone3-host-tests.log` | 60 passed | Real-file editor workspace suite, including asset/prefab Save All, reopen and supported moves |
| `target/milestone3-hidden-reference-tests.log` | 1 passed | Reimport checks referenced meshes in hidden inactive scenes and saved uninstantiated prefabs |
| `target/milestones-compiler-dependencies.log` | 31 passed | Compiler library suite, including dependency closure and isolated-source staging |
| `target/milestones-legacy-prefab-discovery.log` | 3 passed | Compiler discovery handles legacy prefabs outside the conventional prefabs directory |
| `target/milestone3-native-performance-release.log` | 1 passed | Hidden release Vulkan run of the representative 608-entity scene, including genuine GPU timestamps and retained-resource checks |
| `target/milestones-admin-tests.log` | 15 passed | Admin Angular suite |
| `target/milestones-python-tests.log` | 51 passed | Python repository workflow tests |
| `target/milestones-native-template.log` | 1 passed | Opt-in admin test generates and builds a copied native client outside the checkout; build validation, without desktop interaction |
| `target/milestones-release-export.log` | Release package produced | Fresh output outside the checkout includes Player, project and dependencies for all three registered scenes and four referenced models |
| `target/milestones-export-headless.log` | 6 checkpoints passed | Locked exit, key, transition, finish, restart and Stop against the fresh export; every exported file hash remains unchanged |
| `target/milestone3-export-native.log` | 1 passed | Hidden GameApp harness loads that export's manifest/assets and exercises course feedback, Finish, HUD, restart, focus neutrality and camera settling; authored bytes unchanged and no Error log |
| `target/milestones-native-visibility-restore.log` | 1 passed | After the retained empty-model restore fix, hidden Vulkan visibility/reimport checks still reject showing an unprepared hidden placeholder |
| `target/milestones-native-transitions.log` | 1 passed | Hidden native transition/restart resource reuse and rejected destination preparation preserve the current run |
| `target/milestones-native-probe.log` | 1 passed | Vulkan initialization and repeated consistent viewport status |
| `target/milestones-native-launch-contract.log` | 1 passed | Builds/verifies a release executable and checks its path, working directory, Player argument and tamper rejection; does not launch through the OS |
| `target/milestones-native-editor-build.log` | Passed | Final custom-protocol native editor build with the verified production frontend |
| `target/milestones-format-check.log`, `target/milestones-clippy.log` | Passed | Final Rust formatting and Clippy checks; existing Clippy warnings remain |

These native tests use hidden windows and real Vulkan resources. They do not verify OS-delivered interactive
input, WebView/native focus transfer, visible overlay placement or another machine's GPU/driver. The current
editor authoring controls were checked through component/service tests, native DTO serialization and contract inspection; an
interactive desktop authoring session was not performed. Focused counts overlap broader suites and must not be
added to an aggregate as unique tests.
All seven tests ignored by the default aggregate passed in separate opt-in runs listed above.

The final Rust command was:

```text
cargo test --workspace --all-targets --all-features --offline --locked --no-fail-fast
```

The exported-data native test drives the GameApp harness. It does not deliver manual OS input to the exported
release executable. The fresh release package is outside the checkout at
`C:/Users/misha/Documents/Codex/Courtyard Run Milestones Final 2026-11-20`.
The final package includes the retained empty-model correction. Both its six headless checkpoints and the
hidden native route passed with every exported file unchanged.

## Code and review concepts

| Boundary | Source and responsibility |
| --- | --- |
| Authored configuration | `project.rs`, `courtyard-gameplay/src/settings.rs` and `game_camera.rs` validate project payloads and scene follow settings |
| Runtime actions and rollback | `simulation_session.rs`, `world.rs` and `triggers.rs` stage one accepted tick |
| UI ownership | Workspace `game-settings/` contains camera and interaction drafts. `SceneEditingService` retains transaction ownership |
| Asset requests | `asset-authoring.ts` keeps source selection, relative destinations and feedback outside the asset browser's navigation logic |
| IPC guards | `EditorService` carries captured revision and local workspace identity. Native preparation separately verifies its own workspace generation |
| Reusable numeric validation | `core/editor/authoring-number.ts` parses complete f32 drafts without displaying rounding artifacts |

Review each row by following an authored edit through validation, native acceptance, history, persistence,
Play, restart and export. Check malformed payload preservation, missing references, filter incompatibility,
stale asynchronous results and a failure after partial preparation. Record actual test commands and fixtures.
Treat resource counters and timing measurements according to the scope stated in the performance baseline.

## Review findings corrected

| Finding | Correction and evidence |
| --- | --- |
| Windows file rename could replace an occupied publication target | Exclusive `MoveFileExW` publication without replacement; the occupied-target regression preserves both files |
| Reserved Unicode path aliases and lookup used different case rules | Catalog lookup uses the same normalization as reservation; the move/reopen test resolves a Unicode legacy alias |
| A failed temporary write could leave an owned partial file | `write_new` removes the newly created file on write/sync failure while preserving a pre-existing destination |
| A late picker error or queued stale edit could change the replacement workspace's feedback | Picker responses and guarded queue recovery use captured local generation. Tests preserve current feedback and assert no stale recovery IPC |
| Catalog invalidation erased selection and could reuse a pre-publication request | Publication retains the displayed catalog until atomic refresh and detaches older requests; service and real service/panel tests retain the moved selection |
| A deep catalog dependency chain could consume the process stack | Export rejects more than 64 dependency levels and bounds URI work; a long-chain regression checks the rejection |
| Separate reads could parse one catalog version while hashing another | Export parses and hashes one captured source buffer; changed-input checks guard later publication |
| Windows path casing and repeated model URIs could duplicate dependency work | Compiler canonical case keys and deduplicated URI reads keep one dependency entry; model URIs cannot target the reserved asset catalog |
| Legacy prefab discovery and skipped JSON could consume unaccounted work | Discovery covers all of Assets with traversal and I/O budgets, and migration charges JSON reads even when a file is not a prefab |
| A prefab save or asset move could claim an unsaved registered scene path | Host publication reserves every registered scene destination, including retained unsaved drafts |
| Prefab settings could be cloned before their work budget was checked | Shared project/prefab validation checks depth, node and byte limits before clone or serialization |
| Default asset paths required folders missing from a new/bundled project | Import and Save Prefab default to filenames directly under Assets; the component test checks the import default |
| Retained-model lookup rejected a valid renderable model with zero draw parts | Template matching accepts an already-renderable empty model while rejecting an unprepared hidden placeholder. Duplicate/spawn/Undo and hidden missing-model Show assertions remain enforced |

The refactoring keeps numeric parsing pure, interaction/camera draft controls in `game-settings/`, asset file
actions in their own component, and history in the shared edit coordinator. Native modules declare prefab
reference locations explicitly; the generic editor does not guess whether arbitrary strings are entity IDs.
