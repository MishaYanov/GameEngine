# Testing and validation

Run commands from the repository root after installing [prerequisites](usage.md) and both frontend dependency sets.

## Aggregate checks

```powershell
python test_all.py
# Include the opt-in generated native project build:
python test_all.py --native
```

The runner executes all-target Rust workspace tests, Rust documentation tests, admin Angular tests, editor Angular tests without watching, and Python tool tests. It continues with other suites after failure and prints a summary. Exit codes are 0 for success, 1 for any failing/unavailable suite, and 130 for Ctrl+C.

Cargo suites use `--offline --locked`; dependencies must be cached. Nothing is installed automatically. The native option adds the ignored `real_native_template_validation` test, requires SDK/compiler and cached native dependencies, and retains generated output. The launcher UI test can skip when Tkinter or a display is unavailable.

## Focused checks

```powershell
cargo test -p compiler --offline --locked
cargo test -p renderer --offline --locked
cargo test -p engine-logger --all-targets --offline --locked
cargo test -p engine-admin --offline --locked
npm.cmd test --prefix apps/editor -- --watch=false
npm.cmd test --prefix apps/admin-dashboard
python -m unittest discover -s tests -p 'test_*.py' -v
```

See [admin](apps/admin-dashboard/README.md) and [logger](crates/logger/README.md) for package-specific Clippy and smoke commands.

## Coverage and manual checks

Rust tests cover admin collection/processes/jobs/board/generation, logger storage/panics, compiler path validation, and renderer validation diagnostics. Game, runtime, bridge, and editor host currently have no Rust behavioral tests. The editor has no configured end-to-end target. Compilation cannot prove rendering or desktop interaction.

For graphics, check [development and installed modes](apps/game/README.md), texture/depth correctness, resize, minimize/restore, and closure with validation logs. For editor changes, check startup/status, invalid input, installation success, build failure, and launch reporting. For admin changes, verify native process controls, cancellation, project publication, and installer execution as applicable.

The glTF loader is now implemented, but has no dedicated behavioral tests. Manually check the robot primitive, imported texture orientation, UV selection, missing-model errors, and culling. Compiler path tests do not catch the current packaging mismatch: installed output omits `props/robot.glb`, which game startup requires.

The [original test notes](../designs/testing.md) remain a runner/coverage reference. Passing the aggregate command does not establish full behavioral coverage or visual correctness.
