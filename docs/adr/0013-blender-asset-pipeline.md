# ADR-0013: Blender authoring and GLB asset pipeline

- **Status:** Proposed; export integration and reimport are not implemented by this ADR.
- **Date:** 2026-10-27
- **Scope:** Blender-authored static models, export provenance, editor reimport, and build inputs.

## Context

An artist should be able to edit a prop in Blender, export it, and deliberately refresh that asset in the editor
without recreating its scene instances. The engine already consumes glTF, so Blender can connect through a file
pipeline before a live connection is needed. Saving a Blender document, exporting a model, importing it, and adopting
new native resources are separate operations with separate failure cases.

Use Blender as the authoring tool and GLB as the initial exchange format. Keep Blender optional for people who only
open the engine project or build a game from committed exports. A Blender add-on can make export convenient; the
engine remains authoritative for validating and adopting exported assets.

## Current implementation inspected

Inspected commit `279c03ff3b331f79a6cd6a8ecacb27894a3f6600` and the staged, unstaged, and untracked tree on the date
above.
The recorded inspection baseline was already this commit; uncommitted source changes were inspected separately.

| Evidence                                     | Current behavior                                                                                                   |
|----------------------------------------------|--------------------------------------------------------------------------------------------------------------------|
| `crates/runtime/src/assets/catalog.rs`       | Discovers `.gltf` and `.glb` files beneath an asset root.                                                          |
| `crates/runtime/src/assets/gltf_loader.rs`   | Loads geometry and a limited material representation, including base color images.                                 |
| `crates/runtime/src/assets/scene_import.rs`  | Describes static scene hierarchy and transforms; rejects skinned and morph-target nodes.                           |
| `crates/runtime/src/assets/asset_manager.rs` | Caches scenes and native mesh, texture, and model resources. File changes alone do not establish safe replacement. |
| `crates/engine-core/src/asset_registry.rs`   | References use paths and optional numeric mesh indices, rather than persistent authored IDs.                       |
| `crates/engine-core/src/asset_jobs.rs`       | Provides bounded CPU import jobs with cancellation and generation handling.                                        |
| `compiler/src/lib.rs`                        | Prepares and verifies packaged asset inputs; unsupported asset formats are rejected.                               |

These paths do not provide a Blender export button, `.blend` conversion service, or complete automatic reimport
workflow. The existing animation graph UI does not establish native skeletal animation support.

## Dependencies

[ADR-0006](0006-asset-identity-and-import-pipeline.md) owns persistent asset and subasset IDs, dependencies, import
products, and resource lifetime. This proposal specializes that pipeline rather than creating another identity system.
[ADR-0001](0001-editor-command-pipeline.md) and [ADR-0003](0003-rust-typescript-bridge.md) govern editor requests and
diagnostics. [ADR-0004](0004-authoring-play-and-editor-state.md) governs Play revision pinning;
[ADR-0011](0011-persistence-autosave-and-recovery.md) governs durable publication and external-change conflicts.
[ADR-0007](0007-graph-assets-and-execution.md) remains responsible for future animation evaluation.
These dependencies are architectural proposals where their own status says Proposed.

## Proposed decision

### Keep authoring sources and exchange assets distinct

Keep `.blend` files and their authoring dependencies in a version-controlled source-art directory outside the packaged
asset root. Commit the exported GLB files inside the asset root so an engine checkout and release build do not require
Blender. For example, `art-source/props/crate.blend` exports to `assets/props/crate.glb` in a game project. These are
illustrative project-relative paths, not a change to the repository's current asset layout.

The GLB is the engine import source under ADR-0006 and owns the engine asset sidecar and `AssetId`. The `.blend` file
is upstream authoring provenance, not a competing identity for the same runtime model. Record its relative path,
export collection or object selection, Blender/exporter versions, export profile version, and source/dependency
digests in versioned export metadata. Keep machine-specific executable locations in local tool preferences.

Preserve the GLB's sidecar on re-export. Under today's path references, replacing an existing path is only a limited
prototype: it does not guarantee rename safety or stable mesh selection. Production references require ADR-0006's
migration and subasset reconciliation. Never assume that equal names or unchanged mesh indices prove identity after
an export; ambiguous matches block adoption until explicitly resolved. Any future exporter-supplied stable IDs must
have tested duplicate and deletion rules before use.

### Start with a tested static export profile

Use one GLB per exported asset or deliberate collection, with embedded buffers and PNG/JPEG textures. Blender's
exporter supports this container and translates recognized material nodes into glTF material properties; that does
not imply this engine renders every exported feature.
[Blender glTF exporter documentation](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst).

The first engine profile covers triangle meshes, normals, UVs, static hierarchy/transforms, base color factors and
base color textures. Define scale and axis conventions with reference fixtures, using glTF's coordinate convention
without a second importer-side rotation. Include asymmetric geometry, a known-size object, parent transforms, and
negative scale in that proof. Do not label arbitrary Blender shaders, procedural materials, cameras, lights, physics,
skinning, morph targets, or animation as supported by this profile. Reject unsupported required features or report
explicit exclusions before adoption; silent visual success is insufficient. Baking procedural content into supported
mesh and texture data is an authoring step, with its result tested against the same profile.

### Add explicit export and reimport before automatic refresh

The first usable integration provides a Blender **Export to Engine** action and an editor **Reimport** action.
The Blender action wraps a versioned export preset and writes to a temporary sibling file, then publishes the
completed GLB through a tested replacement operation. An export error leaves the previous published file intact.
The add-on needs no persistent network connection and cannot directly mutate engine scenes.

Reimport submits a bounded native job for the selected exported revision. Validate profile compatibility, resource
limits, dependencies, and identity reconciliation before adopting a candidate. Invalidate dependent CPU caches and
coordinate GPU upload and retirement through their native owners; clearing a path cache alone is insufficient.
Publish compact status and diagnostics to the editor. Preserve scene placements, component settings, and authored
overrides; an asset refresh must not rewrite those documents or import Blender objects as new engine objects without
an explicit scene-import operation.

On failure retain the last valid preview, visibly marked with its older revision. Reject cancelled, superseded, or
wrong-workspace completions. Reimport must not silently replace assets pinned by a running Play session; they become
available to a subsequent session. Live Play replacement requires a separate state and resource migration policy.

Only after this path is verified, add an optional watcher on completed exports. Coalesce duplicate events, debounce
bursts, and verify content revisions; filesystem notifications are hints, not proof of a complete file. Reconcile
missed events on rescan and serialize or supersede competing jobs per asset. Watch GLB exports initially, rather than
launching Blender whenever a `.blend` file is saved. Explicit Reimport remains available if notifications fail.

### Keep builds independent of Blender

Builds consume validated, committed exports and capture their revisions before packaging. They fail on invalid required
exports or stale required import products under ADR-0006; an older preview is not permission to ship stale content.
Ordinary builds do not launch Blender or infer that an upstream `.blend` file was re-exported. Export metadata allows
a separate freshness check when authoring sources and dependencies are available; otherwise report that freshness was
not verified. Do not claim that a committed GLB necessarily matches the latest Blender edits.

Optional headless regeneration can follow later as an explicit developer/CI operation with a pinned Blender/exporter
version, a reviewed export script, bounded execution, and recorded inputs. Reproducibility must be measured on fixtures;
do not assume byte-identical GLB output across Blender versions. Installed games contain runtime assets and never need
Blender or source-art files.

## Alternatives and consequences

| Alternative                                  | Tradeoff                                                                                                                                           |
|----------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------|
| Manual GLB export only                       | Works as an initial baseline, but settings drift and repeated steps make iteration error-prone.                                                    |
| Import `.blend` directly by invoking Blender | Convenient discovery, but adds tool installation, process execution, version, and dependency handling to ordinary imports. Defer.                  |
| Persistent live socket connection            | Could support finer updates, but requires session coordination, protocol/version handling, and recovery. Not needed for the initial file workflow. |
| Introduce another model format first         | Adds an importer before proving the existing glTF boundary; reconsider only for demonstrated requirements.                                         |

This decision reuses the current loader and keeps exports reviewable and builds portable. It adds export-profile
maintenance, provenance tracking, and a native replacement path. Static support is deliberately limited; animation and
full material fidelity require their own importer, renderer, and runtime work.

## Implementation sequence and acceptance

1. Establish manual Blender-to-GLB fixtures for the static profile. Verify scale, orientation, hierarchy, UVs, normals,
   base color, and embedded textures in both CPU import checks and the native viewport. Document unsupported features.
2. Implement the versioned export action and explicit Reimport. Prove failed export/import preserves the last valid
   result, scene edits survive refresh, and CPU/GPU resources change safely with frames in flight. Integrate ADR-0006
   identities before promising stable production references; reject ambiguous mesh reordering.
3. Add export provenance and build validation. Prove committed exports build without Blender, source art stays outside
   installed output, changed inputs cannot mix revisions during packaging, and freshness limits are visible.
4. Add opt-in automatic refresh. Test duplicate/missed notifications, partial external writes, rapid successive exports,
   cancellation, project close/reopen, missing files, recovery, and Play revision pinning.

Before accepting implementation, select a supported Blender/exporter version range, freeze the profile and metadata
schemas, and record limits for input size, job time, memory, and refresh latency on representative props/environments.
The add-on packaging location and the need for headless regeneration remain open. Skeletal animation, richer material
support, and a live protocol are future scope. This ADR records a proposal; no Blender, GPU, watcher, or build
acceptance
test was performed by writing it.
