# Project installation pipeline

The editor startup screen asks for a project name and an existing destination folder. Creating a project builds the starter runtime, installs it into `<destination>/<project name>`, and launches its graphics window.

1. Validate the name and destination. Reject reserved Windows names, path separators, relative destinations, and existing project folders.
2. Build `game` in release mode with the workspace lockfile. Reuse the engine build cache.
3. Atomically reserve a new project folder. Copy `game.exe`, texture assets, `project-name.txt`, and a short README. Remove the newly created folder if installation fails.
4. Launch the executable from the installed folder. Report the installed path and any process-launch error separately so the build can still be used.

The form disables submission during the build and displays build failures without discarding user input. Cargo runs on a blocking worker, keeping the editor UI responsive. The current progress message covers the whole build; streamed compiler progress can be added later.

The installed executable reads its project name and assets beside itself. The game now imports `props/robot.glb` in both modes, drawing stationary instances in installed mode and rotating instances in development. The installer still copies only the earlier triangle textures, so fresh output is missing the required model and exits during import. The earlier triangle/cube scene description is superseded; see [current game behavior](../docs/apps/game/README.md). The starter is a runtime stub; it does not generate an editable scene or a separate source project yet.

Development requirements: Rust/Cargo, the engine source workspace, and a Vulkan-capable GPU/driver. This version builds from the development checkout; a distributable installer should ship a prebuilt runtime and avoid requiring Cargo on the user's machine.

CLI installation: `cargo run -p compiler -- "My Game" "D:\Projects"`. The CLI installs only; the editor also launches the result. Reopen an installed project by running `game.exe`.

For CLI examples and desktop dashboard instructions, see [the usage guide](../docs/usage.md).
