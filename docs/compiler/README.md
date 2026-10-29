# Authored project compiler and installer

Source: `compiler`. The tool builds the native game in release mode and packages an authored manifest plus its validated
assets. CPU project/scene types come from engine-core. See [project lifecycle](../project-lifecycle.md) for project
schema 3, shared behavior settings and multi-scene authoring.

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

Project schema 3 identifies catalog entries by stable SceneId, startup_scene_id and scene-scoped behaviors, including
shared configuration references. Inline-only schema-2 files upgrade through ProjectData::from_json. Preparation
captures the asset file list, then validates every catalog scene and its referenced models. A shared resolver requires
each scene/model path to resolve to a regular file within the canonical asset root and to belong to that captured list.
One path index serves the whole catalog pass; scene membership is checked before reading/parsing the file.

Project validation and per-scene behavior target indexing are prepared once for the immutable catalog pass. Parsed
documents already carry scene validation; the remaining cross-document checks verify target membership and packaged
model references. This avoids repeated whole-project work without relaxing containment, file or diagnostic limits.

Captured membership identifies paths. Prepared inputs also retain their byte lengths and fingerprints for the
pre-build recheck and verified copying below. Detailed glTF geometry validation remains with the runtime importer.

The nested build invokes `cargo build --locked --package game --release --target-dir <workspace>/target` using the
source workspace. Cargo and shader compiler glslc must be available. The nested build does not use --offline; an offline
outer invocation does not make it offline automatically.

Installation reserves a new folder with atomic create_dir, copies the executable and prepared assets, canonicalizes the
copied asset root, and runs the same catalog validation pass there before writing project.json plus project-name.txt and
README.txt. This pass includes scene loading, scoped behavior targets and model references. Source projects retain
authored
configuration/assets; output packages contain those data files, not the source workspace. Installed project detection
defaults the game to Player mode. Copy/write failure retains the incomplete output for inspection and returns its path;
existing directories are never merged.

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

This adds one streamed pass over captured inputs without retaining their complete contents in memory. The shared
`copy_verified_asset` and `verify_asset_fingerprint` helpers also keep the same checks during copying after compilation,
so the early recheck does not replace detection of changes made while Cargo runs. Neither pass creates an immutable or
atomic filesystem snapshot, and fingerprints are not cryptographic proof of unchanged data. The scene-report limit
below does not apply to these separate input errors.

## Scene validation diagnostics

Catalog validation visits scenes in ascending SceneId string order and reports one primary failure per invalid scene:
path/read/parse errors first, then behavior-target validation, then model-reference validation. Every scene failure
includes its stable ID and catalog path while retaining the underlying explanation. For example:

```text
Scene "level-a" ("scenes/a.json"): Behavior 0 targets missing entity "missing"
Scene "level-b" ("scenes/b.json"): Model 'models/missing.glb' preflight failed: <filesystem cause>
```

The aggregated scene report retains at most 16 KiB of UTF-8 text. It truncates only at a character boundary, reserves
space for `Further scene diagnostics omitted.`, and continues validating later scenes without adding text after the
notice. The limit applies to the report string, not temporary error allocations, outer installation messages, Cargo
output or total process memory. Project validation and asset-collection failures still return before the scene pass.
Public compiler APIs continue to return string errors; the [typed error design](../../designs/error-handling.md) remains
a separate proposal.

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
fingerprint rejection remains covered. [Testing](../testing.md) records historical evidence; `docs-site/docs-state.json`
identifies the latest scoped TESTER evidence and documentation checks. Copied-root/file-copy checks and successful
compilation are
separate from running the full installer or accepting an installed game.
