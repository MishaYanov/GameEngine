# ADR-0006: Asset identity, import products, and resource lifetime

Status: Proposed; no asset-reference migration or new import cache is implemented by this ADR.

Date: 2026-10-27

Scope: Project assets, imported subassets, dependency tracking, build inputs, and native resource ownership.

## Question and example

If `characters/robot.glb` moves to `actors/robot.glb`, should every scene, animation graph, and prefab need editing?
If a new mesh is inserted into that file, should a reference to mesh 2 silently select a different mesh? We need
identity that survives ordinary authoring changes while retaining cheap runtime lookup.

Recommend persistent asset IDs and stable subasset IDs in authored references, a rebuildable path index, and
versioned derived import products. Runtime handles remain short-lived native implementation details. JSON metadata
describes assets; mesh buffers, textures, decoded clips, and GPU resources stay native.

## Existing implementation inspected

At commit `fcb748b04853209b5e17e680c38c847c19f7f365` and the inspected working tree:

| Area                                           | Current behavior                                                                                                                                                     |
|------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/asset_registry.rs`     | `AssetRef` stores a logical path and optional numeric `mesh_index`. `AssetHandle` contains registry, index, and generation; the registry holds bounded CPU metadata. |
| `crates/engine-core/src/asset_jobs.rs`         | Bounded CPU import jobs provide cancellation and generation-scoped completion handling.                                                                              |
| `crates/runtime/src/assets/metadata_import.rs` | Imports scene descriptors within an asset root; its worker does not upload meshes or decode image pixels.                                                            |
| `crates/runtime/src/assets/asset_manager.rs`   | Native caches retain models, scenes, meshes, and textures; renderer resources are separate from core metadata.                                                       |
| `compiler/src/lib.rs`                          | Captures and checks build inputs, copying assets with size/fingerprint checks. Its `DefaultHasher` fingerprint is not the proposed persistent cache format.          |

There is no persistent ID sidecar database, stable imported subasset mapping, or complete dependency-aware derived
cache in these paths. A generational runtime handle already guards a different problem: stale runtime references.
It does not preserve identity across editor restarts or file moves.

## Assumptions and dependencies

This extends [object composition](0002-object-composition-and-components.md) and the
[Rust–TypeScript contract](0003-rust-typescript-bridge.md). Asset IDs are opaque strings at the UI boundary; they do
not encode file paths or native slots. [Graph assets](0007-graph-assets-and-execution.md),
[game modules](0008-game-modules-and-scripting.md), and [prefabs](0009-prefabs-and-instance-overrides.md) consume the
same identity service. Publication follows [runtime lifecycle](0005-runtime-lifecycle-and-scheduling.md), while
sidecar changes and recovery follow [persistence](0011-persistence-autosave-and-recovery.md).

The first implementation assumes one writable local project and explicit import roots. A shared asset server,
remote cache, and distributed locking are outside this decision.

## Proposed design

### Separate identity, location, and content

Assign a randomly generated UUID `AssetId` once when a source enters the project. Store it, schema version, importer
identity/settings, and subasset reconciliation data in a version-controlled sidecar beside that source. Recommend
`robot.glb.asset.json`; reserve the suffix so import scans cannot treat sidecars as new sources. A rebuildable index
maps IDs to canonical project-relative paths. Moving an asset through the editor moves its sidecar too.

Unity provides a useful precedent: its asset metadata stores a unique identity and import settings, allowing moves
while references retain identity. Losing that metadata breaks the association. Our proposed recovery policy below
is deliberately explicit about that
failure. [Unity asset metadata](https://docs.unity.com/en-us/engine/6000.7/manual/assets-and-media/import-assets/asset-metadata).

Use the conceptual reference `AssetReference { assetId, subassetId? }`. Validate the expected asset kind at the
receiving component or graph port. Display names and paths are query results, not alternate authorities. A content
hash identifies bytes/version, not the enduring identity: editing a texture should not give it a new asset ID.

Treat duplicated IDs as an import conflict. An explicit duplicate operation creates new IDs; a rename preserves them.
Missing metadata must not silently attach an old reference to a new asset with the same filename. Offer restore,
explicit relink, or import-as-new. Never infer identity solely from a matching content hash.

### Source, metadata, and derived products

Keep source files and sidecars as authored, version-controlled inputs. Store disposable import products under an
ignored project cache. Use a versioned cache key containing source digest, importer ID/version, normalized settings,
target profile, and dependency digests. Pin the digest algorithm and canonical settings encoding before persisting
keys. Cache deletion must trigger reconstruction without modifying authored identities or settings.

Record dependencies by ID and required product, including missing dependencies. Maintain reverse edges so a texture
change invalidates its dependent material product. Recompute the affected closure and reject unsupported dependency
cycles with a diagnostic path. Distinguish import-time dependencies from permitted runtime references; a runtime
reference does not automatically require recursive import evaluation.

One source may produce multiple meshes, clips, or materials. Give these stable local `SubassetId` values. Preserve a
mapping using source-provided stable identifiers when available. Names and array positions are insufficient alone.
When an exporter provides no reliable match, report an ambiguous reimport and require mapping or explicit replacement;
do not silently redirect references. Deleted subassets remain identifiable as missing until references are repaired.

### Import and publication

Submit bounded native jobs with workspace generation, asset ID, requested source digest, and importer version. Jobs
write candidates and diagnostics outside the active catalog. Adopt only validated candidates whose request still
matches the current workspace and source. A cancelled job or late completion cannot publish into a reopened project.

Publish a coherent in-memory catalog revision after required dependencies succeed. On failure retain the last good
product for inspection, clearly marked stale; a release build fails on stale required products. Notify Angular with
compact status/invalidation data and query detailed diagnostics on demand. Do not stream source bytes to inspectors.

Initially pin the resolved asset/program revisions used by a Play session until it stops. New catalog products may
serve independent editor previews and subsequent sessions, but cannot silently replace the active Play world's
inputs. Retain pinned products for their consumers' lifetimes. Live Play replacement needs a later explicit reload
and state-migration policy coordinated with ADR-0004, graph execution, and scripting.

Keep CPU import and GPU upload separate. Native resource owners perform uploads at an allowed lifecycle boundary;
old resources remain alive until their actual consumers and GPU submissions finish. Reference counts alone do not
prove GPU completion. The renderer must define fence/deferred-release behavior before live replacement is accepted.
Runtime registry generations reject stale handles; asset revision identifies which product a current handle resolves.

## Alternatives and tradeoffs

Path-only references are simple and match today's files, but rename/reorder safety depends on rewriting every
consumer. A central authored catalog avoids many sidecars but creates a merge hotspot and still needs external-move
reconciliation. Content-addressed identity deduplicates bytes yet treats every edit as a new identity. Sidecars add
files and require disciplined duplicate/move handling, but preserve local ownership and reviewable import settings.

Prebuilding everything simplifies runtime startup but slows iteration. Unbounded on-demand imports reduce initial
work while creating unpredictable stalls. Prefer lazy editor requests with bounded prefetch, plus an explicit build
dependency closure; performance budgets require measurement rather than an assumed universal import strategy.

## Implementation sequence and acceptance

1. Define Rust ID/reference DTOs, metadata schema, duplicate-ID rules, and a read-only index scanner.
2. Add explicit migration from current path/mesh-index references with a reviewable mapping report. Save migrated
   documents only on an explicit operation; unsupported or ambiguous references block migration.
3. Introduce deterministic cache keys, dependency invalidation, and bounded candidate adoption using existing jobs.
4. Integrate graph/prefab/module consumers and build manifests; then implement renderer-safe replacement.

Acceptance requires rename preserving references; duplicate IDs being rejected; mesh reorder never silently
retargeting; missing sidecars remaining diagnosable; cold-cache reconstruction matching warm-cache products; importer
version changes invalidating products; and late/cancelled jobs leaving the current workspace unchanged. Native tests
must exercise resource retirement while frames are in flight. Record import latency, queue limits, retained memory,
and cache hit rates with representative assets.

Open gates are the UUID/digest libraries, subasset matching rules per importer, concrete cache quota/eviction policy,
target profile schema, and GPU retirement implementation. These choices can follow the identity decision; shipped
reference schemas and automatic migration must wait for their fixtures and failure tests. This ADR is documentation,
not evidence that those tests or migration have passed.
