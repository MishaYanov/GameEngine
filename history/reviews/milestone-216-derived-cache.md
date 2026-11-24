# Milestone 216 · Derived asset cache measurement

Keep the existing bounded in-memory caches. Four representative static meshes take 0.59 ms at p95 to decode together, and repeated hierarchy metadata requests already take 0.10 ms at p95. The large textured Robot takes 173.42 ms at p95 to decode and is the clearest candidate for further measurement. An uncompressed product for its first mesh would carry roughly 66 MiB of geometry and pixels before metadata, and verifying its source alone took 25.15 ms at p95. The current evidence measures the cost of a fresh import. To justify a disk cache, a follow-up native capture needs to show repeated slow first presentations for the same large source after the existing model cache has been released.

## Workload and results

On November 24, 2026, the optimized Rust probe ran on Windows build 26200 with an Intel Core i7-13700K. It copied all fixture sources to D: and used isolated D: build, temp and log directories. Each timed group ran after one untimed call, so filesystem pages could already be cached. A cold sample starts without a retained application model or metadata entry. The timings cover CPU work and exclude renderer upload and frame presentation.

| CPU workload | Samples | Median | p95 |
| --- | ---: | ---: | ---: |
| Four static models, ordinary decode | 30 | 0.501 ms | 0.592 ms |
| Four static models, create a fresh in-memory warm batch | 30 | 0.700 ms | 0.776 ms |
| Four static models, read and verify source SHA-256 | 30 | 0.081 ms | 0.085 ms |
| Four static models, hierarchy metadata in a new AssetManager | 30 | 0.389 ms | 0.431 ms |
| Four static models, hierarchy metadata in the same AssetManager | 30 | 0.081 ms | 0.104 ms |
| Textured Robot, ordinary decode of its first mesh | 15 | 160.967 ms | 173.419 ms |
| Textured Robot, read and verify source SHA-256 | 15 | 24.459 ms | 25.151 ms |

| Fixture | Encoded source | Decoded CPU payload | Warm batch |
| --- | ---: | ---: | --- |
| Cinder Oath masonry plus Cube, Capsule and Sphere | 66,232 bytes | 62,592 bytes of vertices and indices | 4 products, 68,952 retained bytes including overhead |
| Cinder Oath first-person sword, knight and warden | 757,100 bytes | Skinned rigs use the animation import path | 0 products |
| Textured Robot | 42,406,956 bytes | 1,800,384 geometry bytes and 67,108,864 RGBA bytes | 0 products, above the warm batch's 32 MiB scratch limit |

In the ordinary scene path, the `AssetManager` also reuses already loaded GPU models. The warm batch does preparatory work before the switch. Its fresh preparation time is distinct from the scene-switch latency it can save. The source-verification timings are one cost a persistent cache would still pay before reading and validating a derived file.

## Source changes and interruption

- Changing one byte in the masonry GLB without changing its size made warm preparation decline the model. Restoring the source restored warm eligibility.
- Changing a vertex in an external `triangle.bin` left `triangle.gltf` byte-for-byte identical. The existing warm path declined this dependency-bearing source. The ordinary loader and the metadata cache after explicit invalidation both returned the changed geometry.
- Cancelling a batch after its first model discarded the partial result and left `asset-catalog.json` unchanged. This path writes no derived asset files. Atomic disk publication and interrupted-write recovery become test cases when a disk writer exists.

## Reproducing the measurement

Copy the rows above to a disposable local project and call `migrate_catalog`. Time `load_first_mesh` for ordinary decode, `PreparedModelBatch::prepare` for a new batch, and `AssetManager::load_scene` with a fresh and retained manager. Use the first mesh in each GLB and SHA-256 with 16 KiB read chunks for the source-verification control. Prime each group once, then record 30 samples for the four static models and 15 for Robot. Use nearest-rank percentiles.

```rust
let prepared = PreparedModelBatch::prepare(&fixture_root, mesh_references.iter(), &|| false)
    .expect("static fixture models are eligible");
assert_eq!(prepared.prepared_count(), 4);
let retained_bytes = prepared.retained_bytes();
```

For a native follow-up, use the matched viewport captures to distinguish a fresh asset manager from one that still owns the model. If those captures show repeated expensive misses, start with one bounded CPU product keyed by source and dependency digests, mesh identity, importer version, normalized settings, and target profile. Measure disk reads and decode, quota and eviction, missing or changed dependencies, corruption, and interrupted publication before enabling it.

## Sources

- [ADR-0006 asset identity and import pipeline](../adr/0006-asset-identity-and-import-pipeline.md)
- [Warm model implementation](../../crates/runtime/src/assets/warm_model.rs)
- [CPU source and metadata caches](../../crates/runtime/src/assets/asset_manager/source_cache.rs)
- [glTF resource and first-mesh loader](../../crates/runtime/src/assets/gltf_loader/resources.rs)
- [Native editor warming experiment](../../scripts/editor_warming_experiment.py)
