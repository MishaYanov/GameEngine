# Persistence, viewport, Blender and MCP verification

Milestones 223–232 were checked on 2026-11-24. The [roadmap](../roadmaps/adr-implementation-223-232.md) records the file-level tasks and sources. The [ADR register](../adr/README.md) records the original decisions.

| Milestones | Commit | Verified behavior |
| --- | --- | --- |
| 223–226 | `87e26a4` | Project Save All reports each scene and manifest outcome. An external edit is preserved and reported as a conflict. Unsaved authored drafts can be reviewed, restored as dirty work, or discarded. |
| 227–228 | `9a4a64b` | The Windows native child viewport hides behind overlapping web menus and dialogs. Viewport shortcuts remain suspended until current visible bounds are acknowledged. |
| 229–230 | `66be502` | The Blender add-on exports static GLB with provenance and stable mesh IDs. The Editor imports the generated asset and preserves its mesh ID on reimport after a geometry edit. |
| 231–232 | `879050c` | Native consent grants short-lived read-only access to the current workspace. A separate stdio worker lists and calls the inspection and asset-query tools. A revoked, expired, or stale grant loses access. |

The documentation, testing, optimization, security, Tauri, Rust and Angular reviews led to checks for clean scenes before publishing the manifest, protection of pending recovery, bounded Save All feedback, viewport input fencing, and accessible MCP grant and revoke controls.

## Test evidence

| Check | Result |
| --- | --- |
| Rust workspace, all targets | 1,384 passed, 25 ignored. This run used a separately built MCP worker. |
| Rust formatting and strict Clippy | Passed. |
| Angular | 786 passed across 98 specs. The production bundle built. |
| Python | 156 passed. |
| Developer site | 28 pages built and link-checked. All eight site tests passed, including history exclusion. |
| Windows desktop | The production Tauri host rendered and presented a scene through Vulkan. Game and Project menus and the Scene Info modal hid the viewport. Closing overlays and restoring the window made it visible again. Moving from 120 DPI to 96 DPI preserved rendering. The host exited with code zero. |
| Blender | The fixture exported in Blender 4.0.2 and 5.3.0. Both outputs passed the native import and reimport test. The packaged add-on also installed, enabled and exported in Blender 4.0.2. |

Local evidence is under `target/adr-implementation/evidence/`, `target/adr-implementation/viewport-qa/run-ZlbL71/`, `target/blender-static-profile/`, and `target/blender-static-profile-5.3/`. The tested add-on archive is `target/blender-static-profile/game-engine-static-glb-v1.zip`.

## Verification limits

- Recovery stores one checkpoint up to 32 MiB per project at its original location and restores authored drafts. Save Copy and cross-process locking are outside the implemented scope. Recovery review and restore still read bounded file fingerprints while holding the workspace lock. Large-workspace latency for that path was not measured.
- The desktop smoke test used one Windows machine with two monitors and one GPU. Manual pointer and IME checks, MCP consent interaction, and clean-machine installer acceptance remain open.
- Blender validation covered GLB export and native import and reimport. Rendering that specific fixture in the interactive viewport was outside this run.
