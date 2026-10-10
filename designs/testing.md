# Repository tests

Run from the repository root:

```powershell
python test_all.py
```

Runs all eight Rust workspace packages (all targets), Rust documentation tests, the independent admin Angular suite, the client Angular suite, and Python tool tests. Each suite runs to completion without watching. Failures remain visible in the final summary and return exit code 1; other suites still run. Ctrl+C stops the run with exit code 130. Missing tools or dependencies count as failures.

Requires Cargo, Node, Python with Tkinter for the launcher UI test, installed frontend dependencies in both Angular projects, and cached Rust dependencies. Rust runs offline with the lockfile. No dependencies are installed automatically. The launcher GUI test explicitly skips if Tkinter or a display is unavailable.

```powershell
python test_all.py --native
```

Also runs the normally ignored native project generation/build smoke test. Requires the Vulkan SDK/compiler and cached native dependencies; it retains the generated temporary project for inspection. It does not interact with a graphics window or verify GPU rendering.

## Coverage limits

Rust tests currently cover admin collection/processes/jobs/board/generation, logger storage and panic safety, compiler destination validation, and renderer validation logging. Game, bridge, runtime, and editor host compile in the workspace test run but currently have no Rust behavioral tests. GPU rendering and interactive Tauri workflows require additional hardware/UI validation. Passing the aggregate command is not a claim of full behavioral coverage.
