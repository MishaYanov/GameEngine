# Authored project compiler and installer

Source: `compiler`. The tool builds the native game in release mode and packages an authored manifest plus its validated
assets. CPU project/scene types come from engine-core. See [project lifecycle](../project-lifecycle.md) for project
schema 2 and multi-scene authoring.

## CLI and library

The compatibility CLI still takes exactly a project name and an existing absolute destination parent:

```powershell
cargo run -p compiler -- 'My Game' 'D:\Projects'
```

It uses the bundled starter configuration, installs without launching, and exits 0 on success, 1 on build/install
failure or 2 on wrong argument count. `install_project(name, destination)` remains the starter adapter. Application
hosts can use:

- `prepare_project(&project, asset_root)` to validate/capture authored inputs into PreparedProject.
- `install_authored_project(&project, asset_root, destination)` or
  `install_project_from_manifest(manifest_path, destination)` to package authored projects.
- `install_prepared_project(&prepared, destination)` to build/copy a previously captured project.
- `project_path(name, destination)` to validate a proposed output without creating it.

Names are trimmed, limited to 80 UTF-8 bytes and allow Unicode alphanumeric characters, spaces, hyphens and underscores.
Reserved Windows device names and existing child paths are rejected. Destination parents must already exist and be
absolute.

## Multi-scene preparation and installation

Project schema 2 identifies catalog entries by stable SceneId, startup_scene_id and scene-scoped behaviors. Preparation
validates every catalog scene and its referenced models against the captured asset file set. Fingerprints detect changed
copied inputs within this contract; they are not cryptographic signatures. Detailed glTF geometry validation remains
with the runtime importer.

The nested build invokes `cargo build --locked --package game --release --target-dir <workspace>/target` using the
source workspace. Cargo and shader compiler glslc must be available. The nested build does not use --offline; an offline
outer invocation does not make it offline automatically.

Installation reserves a new folder with atomic create_dir, copies the executable and prepared assets, revalidates every
copied catalog scene, and writes project.json plus project-name.txt and README.txt. Source projects retain authored
configuration/assets; output packages contain those data files, not the source workspace. Installed project detection
defaults the game to Player mode. Copy/write failure retains the incomplete output for inspection and returns its path;
existing directories are never merged.

The host's build_project additionally requires clean Edit state, no open transaction, matching saved manifest and every
catalog scene matching memory. It releases the workspace lock before compiling. Registering a scene or saving its
manifest does not automatically save dirty scene drafts.

## Packaging bounds and checks

Asset collection validates before copying, orders entries, rejects unsupported file/link types and checks containment
and input changes. Limits include 32 directory levels, 10,000 visited entries, 64 MiB per file, 16 MiB asset JSON and
512 MiB total. The asset JSON cap is lower than the scene parser's 32 MiB input ceiling. These are acceptance limits,
not measured real-time capacity.

```powershell
cargo test -p compiler --offline --locked
```

Tests include project validation, source/output preservation, catalog scene/model checks and changed-input rejection.
This documentation update did not execute engine tests or an installed release. Copied-template checks, file-copy tests
and successful compilation are separate from launching and accepting an installed game. [Testing](../testing.md) records
historical evidence; the documentation checkpoint distinguishes it from current checks.
