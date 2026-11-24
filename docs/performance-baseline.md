# Repeatable engine performance baseline

The performance baseline uses one reproducible scene for CPU simulation and hidden native
rendering. Measurements describe this machine and build configuration; the budgets
are local regression limits, not hardware-independent frame-rate guarantees.

## Fixture and commands

`crates/engine-core/examples/performance_baseline.rs` generates
`target/performance/representative-scene.json` and CPU measurements. The fixture has
608 entities: 576 repeated props, 16 controlled actors and 16 invisible sensors.
It uses three bundled GLB models, 592 visible instances, 68 colliders and a fixed
camera. Sixteen actors move through sensors while the props remain static.
Use a separate emitter fixture to measure particle work.

From the repository root on Windows:

```powershell
./scripts/measure-core-performance.ps1 -EnforceBudget
```

The script builds the release example with the locked dependencies and runs five
fresh processes. Pass `-Offline` when the dependencies are already cached. The
script uses the executable reported by Cargo, including when a different target
directory is configured. Each process
measures 20 scene preparations, warms 60 simulation ticks and measures 600 fixed
ticks. It records process peak working set through the child process handle.
Reports and the generated scene go to `target/performance` by default. Use
`-OutputDirectory` to keep a separate run.

The CPU executable can also run directly, including on other supported platforms:

```text
cargo run --release --locked -p engine-core --example performance_baseline
```

CPU preparation includes JSON parsing, scene validation, module construction and
collision-cache creation. It excludes model decoding, disk-cache misses and GPU
uploads. Tick times include trigger sampling, movement, collision, change tracking
and camera processing. Percentiles use the nearest ranked sample; they do not
average away the slowest ticks.

## CPU results

Five release runs on 2026-11-20, Windows 10.0.26200, Intel Family 6 Model 183,
24 logical processors:

| Measurement | Observed across five runs | Local budget |
| --- | --- | --- |
| Scene preparation p95 | 1.14–1.60 ms | 10 ms |
| Simulation tick p95 | approximately 0.05 ms | 0.5 ms |
| Process peak working set | 6.10–6.20 MiB | 128 MiB |
| Collision-cache rebuilds in 600 measured ticks | 0 | 0 |
| Collider updates / matrix updates | 9,600 / 9,600 | Proportional to the 16 moving actors |
| Trigger edges during measurement | 118 | Deterministic fixture output |
| Serialized scene / world-matrix payload | 274,345 / 38,912 bytes | Recorded separately from process memory |

The detailed machine-readable evidence is `target/performance/core-series.json`;
record CPU and native results separately.
`-EnforceBudget` fails if any run exceeds a budget, memory cannot be observed, or
steady simulation rebuilds the collision cache. Measurements should be repeated
after the final source changes and on intended target hardware.

## Native rendering

The hidden native benchmark uses this same generated scene and the bundled model
assets at 1280×720. It warms 20 frames and measures 120. The release run on
2026-11-20 produced:

| Measurement | Observed | Local regression budget |
| --- | --- | --- |
| Scene/model/GPU preparation wall time | 9.96 ms | 100 ms |
| Frame submission wall median / p95 | 16.62 / 18.06 ms | p95 25 ms |
| GPU device timestamp median / p95 | 0.0582 / 0.0617 ms | p95 1 ms |
| Process working set | 164.85 MiB | 256 MiB |
| Tracked GPU allocation bytes | 4,077,168 bytes | Recorded separately from process memory |
| Cached source models / visible instances | 3 / 592 | Preserve reuse |
| New GPU allocations during 120 steady frames | 0 | 0 |

The 16–18 ms frame wall time includes swapchain presentation waits, CPU culling
and submission. It is distinct from the measured device timestamp duration.
Tracked GPU allocation bytes cover the renderer's instrumentation, not the GPU
driver's entire memory footprint. Process memory is sampled after timed frames.
The native test asserts zero resource growth; the timing and working-set budgets
are reviewed against its report rather than treated as portable unit-test limits.

```powershell
$env:ENGINE_METRICS = '1'
cargo test --release -p game --bin game representative_native_scene_performance_baseline -- --ignored --nocapture
Remove-Item Env:ENGINE_METRICS
```

The generated fixture must exist before this command. `ENGINE_PERF_SCENE`,
`ENGINE_PERF_ASSETS` and `ENGINE_PERF_OUTPUT` optionally select the scene, assets
and output report. The default report is `target/performance/native.json`.

## Interpreting regressions

Compare like-for-like release builds, fixture version, window extent, device and
presentation mode. Keep compilation outside the measurement. Preserve the raw
report and investigate a regression before changing a budget.

The measured CPU path currently stays comfortably within its budget. Its counters
show incremental updates without rebuilding static collision state. This evidence
does not justify a new broad-phase structure, allocator rewrite or renderer
parallelism change. Optimize a demonstrated cost and rerun the same workload; keep
native rendering and GPU ownership measurements separate from CPU simulation.

## Sources

- `crates/engine-core/examples/performance_baseline.rs`
- `scripts/measure-core-performance.ps1`
- `crates/engine-core/src/simulation_session.rs`
- `crates/engine-core/src/collision.rs`
- `target/performance/core-series.json`
- `apps/game/tests/unit/tests_performance.rs`
