# Starter compiler and installer

Source: `compiler`. Cargo package: `compiler`. This development tool builds the native game in release mode and installs a starter executable with textures. The editor calls its library and also launches the result.

## CLI and library

From the repository root, with `D:\Projects` already present:

```powershell
cargo run -p compiler -- 'My Game' 'D:\Projects'
& 'D:\Projects\My Game\game.exe'
```

The CLI accepts exactly two arguments: name and existing absolute destination parent. It installs without launching. Exit codes are 0 for success, 1 for build/install failure, and 2 for wrong argument count. See [setup and usage](../usage.md).

`project_path(name, destination)` validates and returns a proposed child path without creating it. `install_project(name, destination)` validates, builds, reserves a folder, copies output, and returns `Result<PathBuf, String>`.

Names are trimmed, allow Unicode alphanumeric characters, spaces, hyphens, and underscores, and are limited to 80 UTF-8 bytes. Reserved Windows device names are rejected. The destination must be an existing absolute directory; existing child paths are rejected.

## Build and installation

The nested build invokes `cargo build --locked --package game --release --target-dir <workspace>/target` in the workspace embedded from the compiler manifest path. Cargo must be on PATH, the source checkout must remain available, and shader compilation requires `glslc`. The nested build does not use `--offline`; an offline outer command does not force it offline.

After building, atomic `create_dir` reserves the output. Installation copies the platform game executable, the entire nested `apps/game/assets` tree (including `props/robot.glb` and textures), `project-name.txt`, and `README.txt`. The recursive copier accepts regular files/directories and rejects unsupported asset entry types. Copy/write failure attempts to remove only the newly created folder. Existing folders are never merged into.

This installs a runtime with no source workspace or editable scene. The previous missing-robot packaging omission is fixed; generated README wording still describes the older triangle starter, while the executable renders imported robots. For independent sources, use the [admin builder](../apps/admin-dashboard/README.md). See the [installation design](../../designs/project-installation.md).

## Verification

From the repository root:

```powershell
cargo test -p compiler --offline --locked
```

Tests cover unsafe names/destinations, length/whitespace rules, collisions, and copying nested model/texture assets byte-for-byte. The asset-copy test does not build a release executable or launch an installed game. Native installed-run validation remains separate from this file-copy evidence.
