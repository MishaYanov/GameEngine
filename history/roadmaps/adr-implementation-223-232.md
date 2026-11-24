# Persistence, viewport, Blender and Agent milestones

This batch covers the remaining usable work from [0011](../adr/0011-persistence-autosave-and-recovery.md),
[0012](../adr/0012-native-viewport-composition.md), [0013](../adr/0013-blender-asset-pipeline.md), and
[0014](../adr/0014-open-weight-assistant-and-mcp.md). Existing Save All, the single embedded viewport,
managed GLB reimport, and the read-only Agent drawer are the starting points.

## 223. Protect scene and project saves from external edits

Capture the last disk state of each file when opening, registering or reloading it. Check that state before
replacing the file. In `apps/editor-host/src-tauri/src/editor/documents.rs`, preflight every Save All destination
before writing any files, and repeat the check at each replacement. Keep successful baselines on a partial failure.

```rust
verify_disk_baseline(destination, captured)?;
save_authored_document(destination, document)?;
```

Verify external edit and deletion conflicts, registration and Save As fingerprints, and an unmodified retry.
Sources [Persistence decision](../adr/0011-persistence-autosave-and-recovery.md).

## 224. Report Save All results

Use the existing Tauri save path and Editor command queue. Send one result per planned scene or project file,
and present written files and the first blocked file. Reconcile saved flags even when the operation cannot finish.
Keep the existing Save All entry point usable by earlier callers.

```text
Written -> advance the saved baseline. Failed or blocked -> retain the draft.
```

Verify a partial write followed by a successful retry and a stale client response.
Sources [Current project lifecycle](../../docs/project-lifecycle.md) · [Persistence decision](../adr/0011-persistence-autosave-and-recovery.md).

## 225. Write bounded recovery checkpoints

Keep a recovery description and complete authored drafts under the project asset root or a project-owned ignored
location. Use a version, content limits and atomic publication. Skip active grouped edits and Play state. Schedule
checkpoint writes through the current editor queue without holding the UI command queue for unrelated work.

```text
checkpoint = { format_version, open_project_scope, saved_base, committed_drafts }
```

Verify limits, corruption, no changes to authored source bytes, and recovery scope.
Sources [Persistence decision](../adr/0011-persistence-autosave-and-recovery.md).

## 226. Offer recovery after Open

Show the saved recovery draft through the Editor after a project opens. Ask before adopting it as an unsaved edit.
Reject a stale or wrong-project draft and keep disk unchanged. Provide a way to keep the disk version and discard
the recovery copy explicitly.

```text
Open -> inspect checkpoint -> Restore as unsaved drafts or Keep disk
```

Verify that a restored scene is dirty while its disk file retains the original content.
Sources [Editor command flow](../../docs/editor-flow.md) · [Persistence decision](../adr/0011-persistence-autosave-and-recovery.md).

## 227. Verify single-viewport overlay and input ownership

Work in `apps/editor/src/app/modules/editor/workspace/scene-viewport/` and the existing viewport attachment
controllers. Trace overlapping overlays, focus loss, and queued geometry. Keep the native surface suppressed
until the last overlapping overlay closes. Fix concrete failures found by the regression checks.

```text
show viewport = rendered aperture && no covering overlay && current attachment
```

Verify opening and closing overlapping overlays in either order and stale queued work.
Sources [Viewport guide](../../docs/viewport-implementation.md) · [Viewport decision](../adr/0012-native-viewport-composition.md).

## 228. Qualify the Windows viewport

Exercise installed or production-path behavior where the current Windows hardware is available. Keep a repeatable
checklist for overlay focus, minimize and restore, shutdown, and mixed-DPI monitor moves. Report concrete results
and leave hardware-dependent cases open when the required configuration is unavailable.

```text
Open dialog -> clear capture -> close dialog -> fresh input required
```

Sources [Viewport acceptance steps](../../docs/viewport-implementation.md#26-desktop-acceptance-walkthrough) ·
[Viewport decision](../adr/0012-native-viewport-composition.md).

## 229. Pin the static Blender export profile

Add a versioned Blender helper with a bounded GLB validation/publish library and a reproducible asymmetric fixture.
The fixture must exercise axes, transforms, units, and stable mesh extras used by the runtime. Define unsupported
content for the static profile and use the existing managed asset importer for acceptance.

```text
GLB mesh.extras.game_engine_mesh_id -> managed model mesh identity
```

Sources [Asset importer](../../crates/runtime/src/assets/project_assets/mesh_source.rs) ·
[Blender pipeline decision](../adr/0013-blender-asset-pipeline.md).

## 230. Export to Engine from Blender

Add an explicit Blender export action. Publish a self-contained GLB and digest/provenance information without
rewriting scenes or calling Blender from ordinary project builds. Preserve a previously valid export on failure.
Export and explicitly Reimport to verify that a referenced mesh keeps its identity.

```text
Blender -> temporary GLB -> validate mesh identities -> publish -> Editor Reimport
```

Sources [Blender pipeline decision](../adr/0013-blender-asset-pipeline.md) · [GLB authoring guide](../../docs/graphics-and-assets.md).

## 231. Expose read-only live Editor context over local MCP

Add a local stdio MCP entry point and an opt-in scoped desktop grant. Share the typed Agent reads.
Limit inspection to the current granted workspace, reject stale handles, keep JSON on stdout, and revoke the
grant on project replacement. The initial tool set is inspection only.

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

Sources [Agent tool contract](../../crates/authoring-agent/src/tools.rs) · [Agent and MCP decision](../adr/0014-open-weight-assistant-and-mcp.md).

## 232. Verify the MCP handshake and scope

Drive initialize, tools/list, and tools/call with a separate process. Verify unknown methods, malformed and oversized
frames, read-only enforcement, grant expiry, removal of access after switching projects, and reconnection.
Renderer replacement, automatic Blender watching, and model training remain separate project decisions.

```text
old grant + new workspace generation -> access denied
```

Sources [Agent and MCP decision](../adr/0014-open-weight-assistant-and-mcp.md) ·
[MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) ·
[MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) ·
[MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

## Verification

The [implementation record](../reviews/persistence-viewport-blender-mcp.md) lists the commits, test results, and remaining manual checks.
