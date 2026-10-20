# Testing and validation

Run from the repository root after [setup](usage.md). Install both frontend dependency sets and cache Rust dependencies first.

## Aggregate checks

```powershell
python test_all.py
python test_all.py --native
npm.cmd test --prefix docs-site
```

The default runner executes offline/locked Rust workspace tests (all targets), Rust documentation tests, admin Angular, editor Angular without watching, and Python tool tests. It continues after suite failures and returns 0 for success, 1 for any failure, or 130 for Ctrl+C. It installs no dependencies. Tkinter/display-dependent launcher checks can skip when unavailable.

`--native` adds the ignored Vulkan runtime initialization test, generated native-project validation, and the hidden `renderer_smoke` example. These need a supported GPU/window environment, SDK/compiler, and cached native dependencies; generated-project validation retains output. Hidden-window rendering checks execution/assertions, not visual correctness.

## Focused runtime and presentation checks

```powershell
cargo test -p runtime -p renderer -p bridge -p compiler --offline --locked
cargo test -p runtime --test scene_api --offline --locked
cargo run -p game --example renderer_smoke --offline --locked
cargo run -p game --offline --locked
```

Tests moved with assets/scenes into `runtime`. Coverage includes material/image import and validation, scene IDs/removal, shared-model transforms, external public API use, empty model draw lists, reference release, and error causes. Renderer checks include validation logging, device/swapchain selection, and push constants. Bridge serialization and recursive compiler asset copying have tests; editor-host launch results and admin health behavior also have focused coverage. These are scoped checks, not full engine behavioral coverage.

## Completed migration and reported native evidence

The user confirmed that the tests, native smoke, visible game, resizing, and minimizing passed after the runtime move. This completes the five-step sequence: temporary game library, Scene-owned draw construction, shared Transform, remaining runtime modules, and validation. No further feature implementation is required to finish that sequence.

The supplied renderer-ownership notes also report a hidden smoke pass on an RTX 4070, including a 640×360 resized extent and no logged errors. The example covers textured model drawing, request construction, instance removal, GPU idle before releasing cache/model references, zero-size suspension, empty-scene resize, and shutdown. The notes' earlier test count is historical, not a fixed count for the current tree.

These are user-reported native/visual results, distinct from commands rerun during a documentation update. Exact current command outcomes and limits are in `docs-site/docs-state.json`. Installed release execution, clean-checkout reproducibility, and browser visual QA need their own evidence.

## In-progress timing/input/camera checks

`time.rs`, `input.rs`, and `camera.rs` add runtime unit tests for clock boundaries, key-state transitions, and camera math/rotation limits. These modules and game controls are explicitly **in progress**. Prior user-confirmed native extraction checks do not establish that the later controls passed. See [the current guide](timing-input-camera.md) for manual focus/capture/movement scenarios.

## CI draft

The untracked `Jenkinsfile` and `ci/jenkins` files define a local Linux-container Jenkins setup. The draft pipeline fetches locked Cargo dependencies, installs npm dependencies, runs default tests, builds both frontends, checks the docs site, and archives successful build output. It does not run native GPU checks or Windows packaging. The startup helper requires Docker Desktop Linux containers; it has not been launched or validated as part of this docs review. A draft configuration is not evidence of a successful CI run.

See the [original test notes](../designs/testing.md), [runtime](crates/runtime/README.md), and [renderer](crates/renderer/README.md).
