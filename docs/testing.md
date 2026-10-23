# Testing and validation

Run from the repository root after [setup](usage.md). Cache locked Rust dependencies and install frontend/docs
dependencies first.

## Current steps 11–20 evidence

The implementation/tester handoff reports the following final results for the current editing/rendering work:

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
generated scene. The builder still omits JSON scenes from its asset allowlist; generation can compile yet fail runtime
startup.

## Remaining independent checks

Visible native UI/control/lighting acceptance, GPU timestamps/budgets, clean-checkout reproducibility, installed release
execution, Angular/Tauri scene integration, and Jenkins execution evidence remain separate. The Jenkins draft/default
headless pipeline does not exercise native GPU or Windows packaging. Do not infer those results from this report.

See [steps 11–20](editor-steps-11-20.md), [native scene editing](scene-editing.md), [scene persistence](scene-persistence.md),
and [original runner notes](../designs/testing.md).
