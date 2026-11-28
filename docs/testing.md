# Testing and validation

From the repository root, install the dependencies described in [setup](usage.md) and populate the locked Cargo
dependency cache. Use a separate location with available space for Cargo targets, native test output and release
artifacts.

## Run the standard checks

```powershell
python test_all.py
npm.cmd test --prefix docs-site
```

The repository runner executes offline, locked Rust workspace and doc tests, the Angular Editor and Dashboard
suites, and Python tool tests. It continues through failing suites and reports a nonzero exit status on failure.
The documentation check builds the guide pages and tests navigation, local links, search and diagrams.

To check one change while iterating, select its nearest suite.

```powershell
cargo test --locked --offline -p runtime --test editor_session_api
cargo test --locked --offline -p app --lib commands::project
npm.cmd --prefix apps/editor test -- --watch=false
npm.cmd --prefix apps/editor run check:production
```

Editor tests exercise Angular controls and their native command contracts with an injected or mocked transport.
Core and Runtime tests exercise scene validation, history, picking and shared gameplay. The production check
verifies that the web assets compile within the editor's bundle budgets.

## Headless replay

Use `engine-headless` to validate project loading and exercise shared gameplay on the CPU. Run the installed
binary with `--help` for its replay arguments, or run its integration suites from the checkout.

```powershell
cargo test --locked --offline -p engine-headless --test camera_replay --test animation_replay
```

Compare replay hashes within the scope named in each report. The project and render-scene hash covers a
different state projection from the native-component hash. Check the saved source and the staged project
with the same replay when changing packaging or asset identity.

## Orbit camera obstruction checks

```powershell
cargo test --locked --offline -p engine-core --lib camera_probe_math_tests
cargo test --locked --offline -p engine-core --test orbit_camera_schema_api --test orbit_camera_runtime_api
cargo test --locked --offline -p engine-core --test orbit_camera_obstruction_api --test camera_preview_api
cargo test --locked --offline -p engine-headless --test cli orbit_obstruction_replay
```

The camera suites check loaded settings, fixed-step movement, camera traces at the near-plane corners,
blocking and recovery, scene replacement and transactional rollback. Use a visible scene to evaluate
framing and response at different window sizes. [Orbit camera settings](timing-input-camera.md#orbit-camera-obstruction).

## Desktop and release acceptance

Run the native aggregate on a Windows development machine with Vulkan available. Set
`ENGINE_QA_TEMPLATE_ROOT` to an existing directory outside the checkout, such as
`D:\Builds\editor\qa-native-template`, for copied-template compilation.

```powershell
python test_all.py --native
```

This adds native initialization, prepared project compilation and hidden Vulkan examples. Use the
[Windows quickstart](windows-editor-quickstart.md) to build and install the Editor on a Windows test
machine. Exercise New, Open, Edit, Save, Play, Stop and Export Game…. Launch the exported Player from a
separate working directory and check focus, physical input, displayed frames, audio and termination.

## Scene routes, Timelines and streamed regions

From the repository root, run the CPU suites for route activation and stale candidates, Timeline cues and
bindings, additive region ownership, and sampled large-coordinate precision.

```powershell
cargo test --locked --offline -p engine-core --test scene_flow_api --test sequence_api --test world_regions_api --test world_region_precision_api --test region_navigation_api
cargo test --locked --offline -p runtime --lib world_streaming
npm.cmd --prefix apps/editor test -- --watch=false --include src/app/modules/editor/workspace/scene-flow/scene-flow.spec.ts
python -B -m unittest discover -s tests -p 'test_measure_scene*.py'
```

On Windows with Vulkan, run each ignored native test in its own Cargo invocation. Set `ENGINE_SCENE_FLOW_EVIDENCE`
to a writable directory to keep the generated fixtures and grid report. Optionally set `ENGINE_SCENE_FLOW_COMPILER`
to a built compiler executable to exercise staging before the hidden Player test.

```powershell
$env:ENGINE_SCENE_FLOW_EVIDENCE = 'D:\Builds\scene-flow-evidence'
cargo test --locked --offline -p game --bin game cinematic_routes_and_seamless_regions_reach_a_presented_frame -- --ignored --nocapture
cargo test --locked --offline -p game --bin game larger_native_world_keeps_region_resources_bounded -- --ignored --nocapture
```

These tests drive the native Player in a hidden window and check presentation, route and Timeline behavior, and
repeated region residency. Set `ENGINE_SCENE_FLOW_DRAIN_CHECKPOINTS=1` for a comparison run that waits for each
checkpoint draw outside the update timer. The report records checkpoint draw and wait durations and the region
changes in its slowest update. Use a visible exported Player to check how a route feels during normal input, audio
and presentation, and repeat on the intended hardware.
[Interpret load and streaming measurements](performance-baseline.md#scene-switch-and-streaming-measurements).

## Object authoring and Agent package acceptance

1. Build the Editor. Check the packaged Player, `release/agent/authoring-agent-worker.exe`,
   `release/agent/bundle.json` and the `release/agent/engine-help` folder.
2. Open a copy of `apps/headless/tests/fixtures/agent-context-workshop/project.json`. In North select
   the standalone Signal Cap. Open **Agent**, pin the object and choose its mesh attachment in **Ask about**.
3. With model configuration unset, ask how to change its material. Verify **Sources**, **Show Object** and
   **Open Component Editor**. Select the Signal Cap under the Gate Prop in South and verify its address.
4. Change projects during a request. Check the pinned target, citations and conversation belong to the
   current workspace.
5. To check streamed answers, start the Editor with `ENGINE_AGENT_OLLAMA_MODEL` and an available local
   model. Set `ENGINE_AGENT_OLLAMA_PORT` for a different loopback port. Verify **Stop**, responsiveness,
   and the authored Save and Undo history during the request.

[Inspector and Agent](editor-workspace.md#inspector-and-agent).
