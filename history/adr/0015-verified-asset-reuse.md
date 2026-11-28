# ADR 0015 · Reuse verified source assets across projects

Date 2026-11-24  
Status Accepted for source reuse. A persistent cache for decoded assets is deferred.

## Context

The Knight's model and animation clips already exist in a self-contained GLB. A new project should be able to acquire
that exact source and keep building when the donor project is absent. An author also needs to be able to edit the
acquired copy independently. Repeated wall instances within a scene have a different reuse path. They can reference the
same mesh or shared prop while keeping their own transforms and overrides.

The renderer's existing AssetManager retains loaded models, and the static warming path retains bounded CPU products.
The earlier probe measured 0.59 ms p95 for four representative static mesh decodes. It found a textured outlier at
173.42 ms p95 but did not measure a benefit from a persistent cache during native scene presentation. The skinned knight
and warden also take a different loading path from that static probe.

## Decision

1. Acquire a published source using its expected SHA-256 and dependency list. For a self-contained GLB, the list
   consists of the GLB. For a source with external buffers, images or audio, every required file must be pinned and
   checked, with normalized paths relative to the asset root.
2. Copy verified files into the consuming project's asset tree. Record the donor, provenance, digest and destination in
   a project-local reuse manifest. The consuming catalog owns its asset IDs and revisions. Existing project references
   continue to use those local IDs. Export packages the normal project assets.
3. Permit an optional cache of verified source bytes keyed by digest in a user-configured directory. An absent or
   damaged entry is a miss. Verify content before use and publish new entries atomically. Materialize independent
   project copies so authoring writes are isolated. Source refresh is explicit and rejects unexpected changes at the
   destination until the author resolves them.
4. Continue sharing runtime meshes and other existing in-memory products. Consider a persistent cache for a specific
   decoded CPU product only after measurement shows repeated expensive misses. Its key must cover the source and
   dependency digests, importer and product schema versions, normalized import settings and target profile. Bound
   retained bytes, make writes atomic, and treat unsupported, corrupt or evicted data as a miss. Asset identity and
   exported games remain independent of cache presence.

## First adoption

The maze project will reuse the original knight source and record its provenance and checksum. Repeated wall pieces can
reference one project-local source. The first build will verify the reused bytes and package the maze with its own local
copy. A shared acquisition UI and a disk store for derived data are future work.

## Verification

- Verify donor and acquired copies against the recorded digest. A modified or missing source must never silently
  substitute different bytes.
- Open, play and export the consuming project with its own asset root. The output must contain the referenced assets and
  run independently.
- Confirm reimport or local edits affect only the consuming project. Repeated references share a mesh while retaining
  instance transforms.
- For any later persistent derived cache, measure cold and warm first presentation on a named project and test
  dependency edits, importer upgrades, interrupted writes, corruption, concurrent readers and eviction.

## Sources

- [Asset identity and import pipeline](0006-asset-identity-and-import-pipeline.md)
- [Prefab and instance overrides](0009-prefabs-and-instance-overrides.md)
- [Derived asset measurement](../reviews/milestone-216-derived-cache.md)
- [Original authored Knight](../../projects/cinder-oath/assets/actors/knight.glb)
