# Authored project compiler and installer

Source: `compiler`. The tool builds the native game in release mode and packages an authored manifest plus its validated
assets. CPU project/scene types come from engine-core. See [project lifecycle](../project-lifecycle.md) for project
schema 3, shared behavior settings and multi-scene authoring.

## CLI and library

The release-install CLI takes a project name and an existing absolute destination parent:

```powershell
cargo run -p compiler -- 'My Game' 'D:\Projects'
```

An additive stage-only form accepts an authored manifest instead of the bundled starter:

```powershell
cargo run -p compiler --offline --locked -- --stage 'D:\Authoring\My Game\project.json' 'D:\Snapshots'
```

It derives the source asset root from the manifest's adjacent `assets`, runs the same preparation/copy/catalog
validation and creates a new child folder named from the authored project. It does not compile or publish a player;
there is no `game.exe` in stage-only output. Its README distinguishes staged data from a completed executable export.
Existing destinations are rejected and incomplete new output is
retained on failure. This snapshot can be opened or tested with the CPU runner; executable production still uses
the existing finish/install API or editor export flow. [Headless replay](../testing.md#headless-replay).

The name-and-destination form installs Cinder Oath without launching it. The Editor's New Project dialog
offers **Blank project** and **Cinder Oath sample**. To build an existing project with the CLI, use
`--export` with its manifest and destination parent. The CLI exits with 0 on success, 1 on a build
or install error, and 2 for an unsupported argument form.

Applications call the compiler through these entry points.

- `prepare_project(&project, asset_root)` to validate/capture authored inputs into PreparedProject.
- `install_authored_project(&project, asset_root, destination)` or
  `install_project_from_manifest(manifest_path, destination)` to package authored projects.
- `install_prepared_project(&prepared, destination)` to build/copy a previously captured project.
- `project_path(name, destination)` to validate a proposed output without creating it.

Names are trimmed, limited to 80 UTF-8 bytes and allow Unicode alphanumeric characters, spaces, hyphens and underscores.
Reserved Windows device names and existing child paths are rejected. Destination parents must already exist and be
absolute.

## Multi-scene preparation and installation

Project schema 3 identifies catalog entries by stable SceneId, startup_scene_id and scene-scoped behaviors, including
shared configuration references. Inline-only schema-2 files upgrade through ProjectData::from_json. Preparation
validates the optional gameplay module with the same resolver used by the Player, captures the asset file list, and
then validates every catalog scene and its referenced models. Staging writes the captured manifest with its selector. A
shared resolver requires
each scene/model path to resolve to a regular file within the canonical asset root and to belong to that captured list.
One path index serves the whole catalog pass; scene membership is checked before reading/parsing the file.

Preparation indexes behavior targets and validates every scene, including nonstartup scenes. Full scenes use the
Player's gameplay resolver to check required roles, movement and scene destinations. Preflight also checks route
references, Timeline bindings, world regions and distance-based proxy objects.

A streamed region is validated as static additive content in its persistent world. The region document holds static
props and colliders. Its persistent world supplies the camera, player, lighting, audio and active gameplay. Each
entity key must be unique across the world and its regions. Preflight validates these rules for a region without
starting a separate GameSession for it. The export packages full scenes and region scenes with their assets and
preserves the selected gameplay module.

Captured membership identifies paths. Prepared inputs also retain their byte lengths and fingerprints for the
pre-build recheck and verified copying below. Detailed glTF geometry validation remains with the runtime importer.

The source-based export invokes `cargo build --locked --package game --release` from the engine workspace with an
explicit target directory. Set `CARGO_TARGET_DIR` to an absolute path to select it. A relative path is resolved from
the engine workspace, and the default is its `target` folder. Cargo and the shader compiler `glslc` must be available.
The nested invocation has no `--offline` flag. Set `CARGO_NET_OFFLINE=true` in the environment to run it against an
already populated Cargo cache.

Staging creates a new folder, copies the prepared assets, and runs catalog validation against the copied asset
root before writing `project.json`, `project-name.txt`, and `README.txt`. Finish checks the staged manifest and
complete asset inventory before compiling and again before publishing the executable. Publication preserves an
occupied executable path and retains an incomplete export for inspection on failure. A successful export opens
in Player mode by default.

The host's build_project additionally requires clean Edit state, no open transaction, matching saved manifest and every
catalog scene matching memory. It releases the workspace lock before compiling. Registering a scene or saving its
manifest does not automatically save dirty scene drafts.

## Prepared input recheck

`install_prepared_project` validates the destination first, then calls the private `PreparedProject::validate_inputs`
before launching Cargo or creating the output folder. It checks every captured asset path and streams the file through
the existing size/fingerprint checks into `io::sink()`. A stale or missing input returns immediately with its relative
asset path and underlying cause through the existing string-error interface. For example:

```text
Prepared asset "actor.glb" is no longer valid: Asset changed after preflight; build again from the current authored files
```

Preflight and copying stream files through SHA-256 checks. The editor keeps the installed executable's
fingerprint and verifies it and the staged project data on **Run Exported Game**. Choose a destination
that only trusted users can modify. The checks observe the filesystem at specific points, and another writer
can still change files between verification and process start.

## Scene validation diagnostics

Catalog validation visits scenes in ascending SceneId string order and reports one primary failure per invalid scene.
It checks paths and documents first, then behavior targets, model references and gameplay preparation. Each scene
failure includes its stable ID, catalog path and underlying explanation.

```text
Scene "finish" ("scenes/finish.json"): Scene "finish" requires a "finish" binding
Scene "level-a" ("scenes/a.json"): Behavior 0 targets missing entity "missing"
Scene "level-b" ("scenes/b.json"): Model 'models/missing.glb' preflight failed: <filesystem cause>
```

The aggregated scene report retains at most 16 KiB of UTF-8 text. It truncates only at a character boundary, reserves
space for `Further scene diagnostics omitted.`, and continues validating later scenes without adding text after the
notice. The limit applies to the report string, not temporary error allocations, outer installation messages, Cargo
output or total process memory. Project validation and asset-collection failures still return before the scene pass.
Public compiler APIs return diagnostic strings.

## Packaging bounds and checks

Asset collection validates before copying, orders entries, rejects unsupported file/link types and checks containment
and input changes. Limits include 32 directory levels, 10,000 visited entries, 64 MiB per file, 16 MiB asset JSON and
512 MiB total. The asset JSON cap is lower than the scene parser's 32 MiB input ceiling. These are acceptance limits,
not measured real-time capacity.

```powershell
cargo test -p compiler --all-targets --offline --locked
```

Focused regressions cover scene context, primary-cause ordering, deterministic multi-scene reports, UTF-8 report limits,
copied-root validation and captured scene/model membership, alongside project validation, source/output preservation,
unchanged-input acceptance and stale-input rejection for equal-size, resized, missing and nonregular assets. Copy-time
fingerprint rejection remains covered. [Testing](../testing.md) describes the package and replay checks. Copied-root/file-copy checks and successful
compilation are
separate from running the full installer or accepting an installed game.
