# Testing and validation

Run from the repository root after [setup](usage.md). Cache locked Rust dependencies and install frontend/docs
dependencies first.

## Editor authoring polish

Run the workspace integration, Timeline, tab, and graph checks from `apps/editor`.

```powershell
npm.cmd test -- --watch=false --include=src/app/modules/editor/workspace/editor-workspace.editing.spec.ts --include=src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.spec.ts --include=src/app/modules/editor/workspace/animation/timeline.spec.ts --include=src/app/shared/components/graph/graph.spec.ts
npm.cmd run check:production
```

Run the native exit review cases from the repository root.

```powershell
cargo test --locked --offline -p app --lib commands::project
```

The Angular integration exercises the real workspace with a mocked native bridge. Exit review unit tests cover
the native decision and its revision checks. The title-bar click and its dialog flow are separate desktop
acceptance steps. [Milestones 110–121 and browser layout results](editor-authoring-polish.md).

## Editor authoring workflows 122–133

Run the editor tests and production bundle check from `apps/editor`.

```powershell
npm.cmd test -- --watch=false
npm.cmd run check:production
```

Run the native close ownership and exit review tests from the repository root.

```powershell
cargo test --locked --offline -p app --lib commands::project
```

The editor tests exercise cancelled route changes, Assets and Hierarchy actions, undoable Inspector resets,
and exact Timeline navigation. Native unit tests cover close registration and teardown decisions. Test the physical
Windows title-bar button on desktop for end-to-end confirmation. [Milestones 122–133 and Timeline ruler evidence](editor-authoring-workflows-122-133.md).

## Camera authoring and host parity

```powershell
cargo test --locked --offline -p app --lib camera_preview
cargo test --locked --offline -p app --lib camera_room_preview
cargo test --locked --offline -p runtime --test game_content_rect
cargo test --locked --offline -p engine-headless --test camera_replay --test animation_replay
npm.cmd --prefix apps/editor test -- --watch=false
```

The camera-room subprocess checks intermediate poses, blockers, queued inputs, restart and tick partitioning across
source and staged projects. The editor-host test exercises a saved camera through the native undo stack, reopen and
Play. Release export, pixel comparisons and timings have their own reproduction
commands. [Camera verification](milestone-10-camera-review.md)

## Orbit camera obstruction checks

Run the geometry, session and preview checks from the workspace root.

```powershell
cargo test --locked --offline -p engine-core --lib camera_probe_math_tests
cargo test --locked --offline -p engine-core --test orbit_camera_schema_api --test orbit_camera_runtime_api
cargo test --locked --offline -p engine-core --test orbit_camera_obstruction_api --test camera_preview_api
cargo test --locked --offline -p engine-headless --test cli orbit_obstruction_replay
```

The unit tests exercise contact math and candidate accounting. The schema and runtime suites cover the
projection-dependent minimum radius, fixed-step input and target lifetime. The obstruction suite checks
all four near-plane corners at 3:1, target and wall motion in the same step, and recovery when a timed wall despawns.
It also checks subtree and layer filtering, restart, scene replacement and rollback when sensors and the camera
exhaust their shared budget on the second fixed step. The preview suite compares accepted poses with Play
and checks that a rejected preview sample preserves the previous pose. The CLI test launches the headless executable
with a thin wall beside the center ray. It checks the eye at admission, one fixed step after a 90° turn, and after
recovery to the requested distance. These are CPU-level checks.
Interactive framing at different window sizes and exported-player presentation belong to native acceptance.
[Orbit camera settings and collision behavior](timing-input-camera.md#orbit-camera-obstruction).

## Milestones 7–9 verification scope

The [M7–M9 review](milestones-7-9-review.md#run-and-verify) documents the current event, character and animation
contracts and focused commands. Its [verification ledger](verification/milestones-7-9-tests-2026-11-21.json)
records integration results; older totals below remain evidence for their original snapshots.

Headless now decodes animation assets on the CPU while creating no GPU/window. Its existing `state_hash` retains
the `project_and_render_scene` scope. `native_components_hash` separately covers full lifecycle/event/controller/
animation diagnostics under `native_components_v1`; opaque game-module state, game progress, scheduler and trigger
history are excluded. Native inspection paging does not truncate this projection. Compare each hash only within
its declared scope. CPU replay, hidden Vulkan tests, native packaged execution and visible controls are separate checks.

## Current editor workspace documentation scope

The 2026-11-20 reconciliation inspects `e0f2148`, history since `85269c8`, and staged/unstaged/new source including
module/role authoring, Collider Inspector, sensors/filtering and resource/cache changes. The
[building-blocks review](building-blocks-review.md#validation-evidence-and-limits) attributes current focused checks,
final completion artifacts and remaining native limits. `docs-site/docs-state.json` records the inspection baseline
and hashes; it is not certification of an index-only snapshot. Older totals below remain historical.

Current completion evidence includes 734 workspace all-target/all-feature Rust passes with five intentional skips,
Rust documentation checks, 401 editor tests across 43 specs, production-bundle success with the existing setup
stylesheet warning, 15 admin tests and 51 Python tests. Hidden real-Vulkan probe, transition/restart/resource-reuse,
renderer/resize/zero-size and exported-course execution are separately attributed in the review. The exported-course
test drives production GameApp code against packaged data and dispatches focus events directly; it does not prove
OS keyboard delivery or interactively drive the standalone exported executable. Targeted follow-up checks and
documentation-site publication results belong to the completion report/checkpoint, not these historical totals below.

## Current project lifecycle changes

The [project lifecycle guide](project-lifecycle.md) covers project schema 3 / editor protocol 2: root validation,
retained drafts, scene-aware history, save/reload policies and catalog-wide build/headless preparation. Focused tests
exist in engine-core/tests/multi_scene_api.rs, host editor_workspace.rs, frontend editor.service.spec.ts and
compiler/headless tests. Documentation maintenance executes site checks; current application evidence is separately
attributed in the completion review. Earlier engine counts below remain historical.

## Historical steps 21�35 verification

The [learning report](editor-steps-21-35.md#verification-and-remaining-work) records the current Angular/runtime/host
evidence and scope. Implementation/TESTER handoffs report:

- Workspace all-target offline/locked tests: **294 passed, zero failed, two ignored**. TESTER added 62 Rust tests; the
  copied-template native test also passed in a separate explicit run.
- Angular editor suite: **13 passed**, including 10 new service tests. Production build and strict TypeScript pass.
- Both default ignored checks passed in separate explicit runs: native Vulkan initialization (**one passed**) and
  copied-template native validation (**one passed**, manifest/artifacts checked). Both hidden-window examples (
  `renderer_smoke`, `scene_io_smoke`) pass. The renderer smoke verifies 257 compatible instances in one draw and
  completed GPU timestamps, plus resource budgets/lifecycle.
- Workspace all-target check, ordinary game/app build, Clippy and targeted formatting checks pass, with inherited
  lint/cache warnings.
- SECURITY closed both resource P2 findings and failed-import cleanup concern in static re-review. LOGGER added targeted
  editor/viewport boundary logs and passed the host check.

Documentation maintenance separately rebuilt/link-checked 27 pages, passed all five site tests, resolved 416 anchor
targets and checked documentation whitespace. It did not modify production code or tests. Bundled scene content remains
unchanged. Exact inspection baseline and verification limits live in `docs-site/docs-state.json`.

Angular build/strict TypeScript checks and CPU contract tests do not prove that a UI layout was composed or that the
Windows child pane was interactively accepted. Hidden GPU/native examples, when run, are execution evidence distinct
from visible input/appearance checks. GPU timestamp support/availability is not a utilization or speedup result.

## Historical steps 11–20 evidence

The implementation/tester handoff reports the following final results for the earlier steps 11�20 editing/rendering
work:

| Check                                                  | Result and scope                                                                                                        |
|--------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------|
| Runtime/game/renderer all-target tests, offline locked | **183 passed, zero failed, one ignored**; includes final callback regressions and non-finite drag validation            |
| Explicit ignored Vulkan runtime initialization         | **One passed** when enabled; separate from the default suite's ignored count                                            |
| Hidden renderer_smoke                                  | Passed textured rendering, resize, suspension, resource release and shutdown                                            |
| Hidden scene_io_smoke                                  | Passed save/reload, rollback, GPU lifecycle, and unchanged bundled scene                                                |
| Debug game build                                       | Passed; final source also built in target/steps-11-20-verification after ordinary-target Windows access/file-lock error |
| Full workspace all-target compile                      | Passed                                                                                                                  |
| Focused Clippy runtime/game libs, bins, examples       | Completed with existing warnings; not a zero-warning claim                                                              |
| Documentation site                                     | Local result recorded in docs-site/docs-state.json                                                                      |

Keep the standard test total separate from the explicit native invocation and two example executions. Historical totals
in earlier documentation checkpoints describe earlier source. Native hidden examples execute GPU/window paths and
assertions; they do not verify what the rendered image looks like.

The user explicitly chose automated-only verification for this batch. No visible panel inspection, mouse/gizmo
interaction, lighting appearance comparison, or GPU-performance measurement was performed. Earlier user-confirmed
resize/minimize results apply to the prior renderer/runtime migration and do not substitute for this batch's visual
acceptance.

## Commands

```powershell
cargo test -p runtime -p game -p renderer --all-targets --locked --offline
cargo check --workspace --all-targets --locked --offline
cargo build -p game --locked --offline
cargo clippy -p runtime -p game --lib --bins --examples --locked --offline
cargo test -p runtime --locked --offline native_runtime_initializes_and_reports_consistent_status -- --ignored --nocapture
cargo run -p game --example renderer_smoke --locked --offline
cargo run -p game --example scene_io_smoke --locked --offline
npm.cmd test --prefix docs-site
```

The ordinary-target replacement error does not prove a particular running process held the file; no process was
terminated. The isolated build provides final-source compile/artifact evidence without claiming the ordinary game.exe
was replaced by that command.

Clippy completed with existing frame argument-count, glTF modulo, example collapsible-if, and cache-hardlink warnings.
Successful compile does not prove every workspace test has been rerun.

## Coverage and test organization

Runtime unit/integration tests cover import/material/image validation, primitive geometry, persistent keys/schema
migration, scene snapshot restoration, mixed history, selection, bounds, ray picking, gizmo math, asset discovery,
render-data contracts, and conservative visibility. Game callback regressions cover Edit/Play isolation, authored
saving, failure preservation, input/drag/field interactions, and dirty-content behavior. Renderer checks cover
vertex/push layouts, validation logging, and device/swapchain selection.

Relevant public-API tests are under `crates/runtime/tests`: `editor_api.rs`, `picking_api.rs`, `scene_history_api.rs`,
`gizmo_api.rs`, `catalog_api.rs`, and `render_data_api.rs`. Adjacent `tests_*.rs` modules remain included under cfg(
test); files should not be run as independent applications. Tests were owned by TESTER during this implementation/docs
sequence.

Finite-data/NaN drag rejection, camera-parallel handles, command failure/no-op redo preservation, restored
identity/bindings/order, and shared Arc model references are important regressions, not a complete native-interaction
proof. Hidden examples supplement CPU tests with shader/pipeline/layout and lifecycle execution on actual Vulkan
resources.

## Aggregate repository runner

```powershell
python test_all.py
python test_all.py --native
```

Default suites are offline/locked all-target Rust workspace tests, Rust documentation tests, admin/editor Angular tests
without watching, and Python tool tests. They continue after failures; exit codes are 0 success, 1 failure, and 130
Ctrl+C. Dependencies are not installed automatically. Tkinter/display-dependent tests can skip.

The current native option adds runtime initialization, generated-project validation, renderer_smoke and scene_io_smoke.
Native generated-project compilation retains output and is not the same as launching the
generated scene. The JSON asset omission is fixed with bounded validation; generation compilation still does not
establish final installed runtime acceptance.

## Object authoring and Agent package acceptance

Use separate D: locations for Cargo output, the native template test and the release. Template generation copies the
workspace and requires a destination outside the source checkout. For example, set `ENGINE_QA_TEMPLATE_ROOT` to an
existing `D:\Builds\editor\qa-native-template` through PowerShell before running the native aggregate.

1. Build the Editor with `apps/editor-host/build-windows-release.ps1`. Check that the portable folder includes
   `release/player/game.exe`, `release/agent/authoring-agent-worker.exe`, `release/agent/bundle.json`, and
   `release/agent/engine-help`. Validate the same files in the installed layout on a Windows test machine.
2. Launch the Editor from an unrelated working directory with Cargo and the engine checkout removed from PATH. Open
   a copy of `apps/headless/tests/fixtures/agent-context-workshop/project.json`. In North, select the standalone
   Signal Cap. Switch the right drawer to **Agent**, pin the object and choose its `mesh` attachment in **Ask about**.
3. Ask how to change the selected object's material. With the model configuration unset, check **Sources** and the
   keyword completion status. Change the Hierarchy selection while a request is pending, then use **Show Object** to
   return to its original target. **Open Component Editor** focuses that attachment or opens its specialist pane.
4. Repeat in South for the Signal Cap under the Gate Prop. Compare the names and native addresses. A later question
   about a second project must not retain the pinned target, sources, or conversation from the first project.
5. With a separately configured, local Ollama model, set `ENGINE_AGENT_OLLAMA_MODEL` before starting the Editor. If the
   service uses a different loopback port, also set `ENGINE_AGENT_OLLAMA_PORT`. Check the streamed text, **Stop**,
   and authoring responsiveness while a request is pending. Confirm that Save and Undo history are unaffected by the
   query. To verify offline behavior again, unset the model and reopen the Editor.

The packaged help index uses version-matched authoring guides and approved project manifest metadata. The worker runs
separately from the Editor, and requests bind to the authored object address and revision. Keyword lookup and optional
embedding ranking have deterministic coverage. The tests do not need a model download.
[Agent workspace usage](editor-workspace.md#inspector-and-agent) · [Windows Editor and export workflow](windows-editor-quickstart.md)

The November 2026 package run exercised native primitive creation, the Agent drawer, offline citations, exact object
navigation, hierarchy dragging, save and reopen, and the exported Player from a separate working directory. A 12,000
object scene mounted 19–24 hierarchy rows. Seventeen Arrow Down inputs that
changed focus had a p95 of 44.1 ms measured through two WebView animation frames. The native host
kept one viewport at 800×580. The NSIS installer was built and its resource manifest checked. Clean-machine
installation and a real local model remain separate checks.

## Remaining independent checks

Current visible native UI/control/lighting acceptance, measured performance budgets, clean-checkout reproducibility,
another-machine installed-release execution and Jenkins execution evidence remain separate. Historical composed
Angular/Tauri and exported-player smoke has fixture-specific limits; it is not repeated by current CPU or hidden
Vulkan tests. The Jenkins
draft/default
headless pipeline does not exercise native GPU or Windows packaging. Do not infer those results from this report.

See [steps 11–20](editor-steps-11-20.md), [native scene editing](scene-editing.md), [scene persistence](scene-persistence.md),
and [original runner notes](../designs/testing.md).
