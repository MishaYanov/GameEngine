# Testing and validation

Run from the repository root after [setup](usage.md). Cache locked Rust dependencies and install frontend/docs
dependencies first.

## Current steps21–35 verification

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

The current native option adds runtime initialization, generated-project validation, and renderer_smoke. scene_io_smoke
is invoked separately. Native generated-project compilation retains output and is not the same as launching the
generated scene. The JSON asset omission is fixed with bounded validation; generation compilation still does not
establish final installed runtime acceptance.

## Remaining independent checks

Visible native UI/control/lighting acceptance, measured performance budgets, clean-checkout reproducibility, installed
release
execution, composed Angular/Tauri pane acceptance, and Jenkins execution evidence remain separate. The Jenkins
draft/default
headless pipeline does not exercise native GPU or Windows packaging. Do not infer those results from this report.

See [steps 11–20](editor-steps-11-20.md), [native scene editing](scene-editing.md), [scene persistence](scene-persistence.md),
and [original runner notes](../designs/testing.md).
