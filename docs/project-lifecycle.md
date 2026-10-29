# Project lifecycle and multi-scene authoring

The current working tree adds project-root ownership and multi-scene authoring to the CPU backend, host and Angular
service. This guide describes the inspected source, including uncommitted changes; it does not certify native
interaction or the current engine test results. [PROJECT3 handoff](project3-handoff.md) remains the historical steps
36–45 report and original next-step prompt.

For the method-by-method path from UI startup through save/reload and packaging, see
[Editor flow: startup to project builds](editor-flow.md). Its diagrams also distinguish admin Cargo rebuilds from
new project installations and native viewport reconstruction.

## Project format and scene identity

Project manifests now use **schema 3**, while individual scene documents also use **schema 3**. These versions are
independent of **editor protocol 2**. `ProjectData::from_json` upgrades a schema-2 manifest containing only inline
behaviors to schema 3 in memory; subsequent serialization writes schema 3. Schema 2 cannot contain
`behavior_definitions` or shared behavior attachments, even if the definitions list is empty. Unsupported versions,
including schema 1, are rejected. Older scene schemas still use the scene parser's existing migration behavior.

```json
{
  "schema_version": 3,
  "name": "Example Game",
  "scenes": [
    {"id": "main", "asset_path": "scenes/main.json"},
    {"id": "menu", "asset_path": "scenes/menu.json"}
  ],
  "startup_scene_id": "main",
  "fixed_hz": 60,
  "max_catchup_steps": 8,
  "behavior_definitions": [
    {"id": "walk", "settings": {"kind": "controlled_move", "speed": 1.0, "space": "world"}}
  ],
  "behaviors": [
    {"scene_id": "main", "behavior": {
      "kind": "shared", "entity": "actor", "definition": "walk"
    }}
  ]
}
```

Both JSON scene files must exist, and `main` must contain the persistent entity key `actor`. Scene IDs are distinct from
entity keys and filesystem paths. `SceneDescriptor` validates a relative JSON asset path; `SceneCatalog` rejects
duplicate IDs/paths and allows up to 64 scenes. IDs are nonempty, at most 256 bytes, with no whitespace/control
characters. Catalog serialization is ordered by ID. Scene-scoped behaviors are checked against their own scene, so
identical entity keys in different scenes do not share behavior accidentally.

Shared definitions reuse settings across entity attachments and scenes. `BehaviorDefinition` has a unique `id` and
`BehaviorSettings` of `controlled_move` or `spin`; each `shared` attachment supplies its own entity key and references
that ID. Inline behaviors remain supported. Resolution produces entity-specific `ResolvedBehavior` values before
constructing the configured module and its controlled-target cache. A definition shares configuration, not mutable
runtime state. It is not a nested behavior graph, script or general component resource.

There are at most 1,024 behavior attachments and 1,024 definitions. Definition IDs are nonempty, at most 256 bytes and
contain no whitespace/control characters. Duplicate IDs, unknown references and invalid settings fail validation;
each attachment's entity must exist in its own scene. Changing shared configuration marks scenes containing shared
attachments as affected conservatively, even when their attachment records did not change. Project history accounts
for the retained definition/attachment data within its existing memory budget.

Manifests remain bounded to 1 MiB. Retained authored/saved scene documents have a 256 MiB conservative logical budget
and a 64-document count ceiling, including documents retained for history. That document budget excludes selection
vectors, history storage and allocator overhead; it does not measure total process or GPU memory. Each scene retains
its existing individual validation and size limits, and each selection is separately limited to 4,096 distinct existing
entity keys.

## Open a project or standalone scene

`ProjectLocation::from_manifest` in the Tauri host requires a regular, nonsymlink file named `project.json`. Its
canonical parent is the project root. The canonical `assets` directory must exist and remain within that root. Host
scene operations additionally resolve paths within the asset root and reject invalid aliases/links according to their
operation checks.

`editor_open_project` prepares and validates a candidate before replacing the active workspace. It loads the catalog's
documents and starts at `startup_scene_id`. Candidate failure preserves the previous workspace; replacement advances
revision identity, scopes new jobs and invalidates viewport state. Opening another project is not a save operation: the
UI owner must decide how to handle unsaved work before explicitly replacing it.

The compatibility `editor_open(assetRoot, scenePath)` can create a standalone scene workspace when no manifest exists.
If a manifest exists, opening must agree with its configured startup scene. Successfully saving project settings
promotes standalone state to manifest-backed state. A manifest-backed workspace does not silently become standalone when
its manifest disappears; reload fails and retains memory state. A standalone workspace adopts a discovered manifest only
after successful validation/reload.

## Documents, switching and history

`EngineBuilder::with_scenes` supplies inactive documents. `EditorSession` retains active and inactive authored/saved
drafts together with each scene's transient selection. Snapshots now include `active_scene_id`, `project`,
`project_dirty` and sorted `scenes` entries containing `id`, `asset_path`, `loaded` and `dirty`.

- `add_scene` adds an explicit ID, new relative path and document. In the host it requires a new file path; it does not
  write the document automatically. Save the scene before persisting a catalog that references it.
- `switch_scene` requires Edit mode and no open transaction. It retains dirty drafts and the outgoing selection, then
  activates the incoming document with that scene's retained selection. The incoming selection is validated before
  activation. Navigation is not itself a history edit.
- `set_startup_scene` changes which scene the player starts with; it does not switch the editor's active scene.
- `remove_scene` requires an inactive, nonstartup, clean scene. It removes catalog membership and that scene's
  configured behaviors, without deleting its disk file. History may retain its document for undo.
- `change_scene_path` changes catalog metadata. The host requires an existing file containing the authored document and
  rejects another scene's resolved path; use Save As to create a new destination.

History records retain scene identity and historical selection. Undo/redo can activate the scene belonging to the target
edit, even after navigation, restoring the selection recorded for that edit while preserving other drafts. Restored
selection is validated before consuming the history record or activating its scene. Scene/catalog/project changes use
the shared bounded history; open transactions and Play restrict structural operations. The overall dirty state includes
project settings and inactive scene drafts, not only the displayed document.

Selection retention lasts only for the current session. Newly created or freshly loaded scene drafts start with empty
selection, and reopening a workspace starts empty again. Selection is not serialized into scene/project files and does
not contribute to scene/project dirty flags or change saved baselines.

## Editor Play and simulation scene identity

`SimulationSession::scene_id()` identifies the scene being simulated separately from the project's authored
`startup_scene_id`. `new(project, document)` and `with_factory(project, document, factory)` select the startup scene.
`with_factory_for_scene(project, scene_id, document, factory)` selects an explicit catalog scene and validates its
document before creating the gameplay module or runtime world. The project configuration keeps its startup setting.

Editor `StartPlay` creates a `GameSession` around that simulation with the active scene ID and authored document. If the
startup scene is A and the editor is displaying B, Play runs B while the project still names A as its launch default.
Configured behaviors and movement-input validation both use the running scene ID, so equal entity keys in A and B do
not select A's behavior configuration while playing B. Stopping Play returns to the retained authored state.

Scene identity stays fixed within each `SimulationSession`. `GameSession` can explicitly prepare and replace that
simulation without rewriting the authored editor scene or startup setting. `prepare_scene(scene_id, document)` builds
and validates a candidate CPU world/module while the old simulation stays installed. Preparing the already active
scene is rejected. `commit_scene(prepared)` accepts only a candidate created by that GameSession at its current
generation; another successful commit makes older candidates stale. Commit preserves the current pause state, clears
input, resets simulation state with the new candidate and advances the scene generation.

EditorSession exposes this through `play_scene_id`, `prepare_play_scene` and `commit_play_scene`. Preparation and commit
require Play, no open transaction and revision capacity. Commit advances command and scene revisions; it does not edit
authored documents or switch the authoring catalog selection. Stopping Play returns to the retained authored state.
These Rust APIs do not add a typed scene-transition command or Angular control. GameSession now retains a game-owned
progress value across ordinary scene changes; the outgoing simulation world is still discarded. Progress is in memory,
not a save file or automatic scene-trigger system. The native Player adds an explicit
[F8 catalog cycle](apps/game/README.md#player-scene-switching) using this boundary.

`prepare_new_game` prepares a fresh simulation and factory-created progress, including when restarting the active scene.
Committing that candidate resets progress, scene generation and session identity, invalidating all earlier prepared
tokens; it starts with the new simulation's pause state. Ordinary scene commits preserve the latest progress and pause
state rather than copying progress captured during preparation. Stopping Play drops progress; starting Play creates
another run. See [game progress](crates/runtime/README.md#game-progress) for typed actions and transaction limits.

## Save, Save As, register and reload

| Operation               | Persistence and limits                                                                                                                                                                                                 |
|-------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `editor_save`           | Saves only the active authored scene and its scene baseline; no project or inactive-draft save. Rejects an open transaction.                                                                                           |
| `editor_register_scene` | Validates an existing contained JSON file, rejects registered aliases, allocates a scene ID, and records a clean document. Catalog changes still need `save_project`.                                                  |
| `editor_save_scene_as`  | Requires Edit/no transaction and a new destination. Keeps scene identity, writes the active authored document and changes its catalog path; leaves the old file intact. The project manifest remains dirty.            |
| `editor_save_project`   | Validates all loaded documents and existing distinct catalog files, writes the manifest, and marks only the project baseline saved. It does not save dirty scene drafts.                                               |
| `editor_reload`         | Replaces the validated workspace from disk. Omitted policy defaults to `require_clean`; `discard_unsaved` explicitly permits discarding edits. Both reject an open transaction; clean-policy reload also rejects Play. |

Rust callers use `EditorWorkspace::reload_with_policy`; the older implicit-discard `reload` method has been removed.
Angular `EditorService.reload()` sends `policy: 'discard_unsaved'`; a UI must not present it as a harmless refresh.
`refresh()` requests the current in-memory snapshot instead.

Writes are separate file operations, not a multi-file transaction. Save As can leave a new unreferenced file if the
later catalog commit fails; it preserves the old file/authored state on that failure. Undo changes document/catalog
history, not physical disk writes. After Save As, save the manifest separately. Save dirty scenes individually before a
build; saving the manifest alone cannot make those documents clean.

## Angular and host contract

The host advertises editor protocol 2 / scene schema 3, and `EditorService.loadCapabilities()` requires that pair.
Models mirror project schema 3, scene descriptors, scene-scoped/shared behaviors and expanded snapshots. Existing
widgets still
need to consume these APIs; no completed scene-browser layout or interactive acceptance follows from service additions.

```typescript
await editor.loadCapabilities();
await editor.openProject(manifestPath);
await editor.registerScene('scenes/menu.json');
const menu = editor.snapshot()!.scenes.find(s => s.asset_path === 'scenes/menu.json')!;
await editor.switchScene(menu.id);
// Use validated entity/transform commands, then save this scene if changed.
await editor.save();
await editor.saveProject();
```

Additional service helpers are `saveSceneAs(path)` and `saveProject()`. `set_startup_scene`, `remove_scene`,
`change_scene_path` and `set_project` use the typed `execute` path and its revision checks. Host command registration
includes register/Save As. Structured backend errors already exist in the coordinator, but many host boundaries still
return strings; the [error-handling design](../designs/error-handling.md) is a proposal, not a completed workspace-wide
migration.

## Build and player/headless behavior

Host build preparation requires clean Edit state, no grouped edit, a manifest matching memory and **every catalog scene
** matching its retained document on disk. Compilation runs after releasing the workspace lock. Compiler preparation
requires catalog scenes and referenced models to belong to the captured asset list; copied output uses the same
validation pass against its canonical asset root. Scene failures include ID/path context in stable ID order, with one
primary cause per invalid scene and a 16 KiB UTF-8 report limit. See [compiler diagnostics and packaging
checks](compiler/README.md#scene-validation-diagnostics) for the report scope and captured-input limits. Admin
native-template validation also traverses the catalog. Detailed geometry import remains a runtime concern.

Player launch resolves the configured startup ID to a scene path; configured simulation uses that scene's behaviors.
Headless project loading prepares all catalog documents through `with_scenes`, rejects file aliases/budget overflow and
supports typed scene commands in replay. A standalone headless scene gets a stable transient ID for comparable
diagnostic hashes. The native Player's explicit F8 cycle is separate from headless authoring scene commands and does
not provide automatic gameplay transitions.

## Source map and verification

| Source                                                                                              | Role                                                                         |
|-----------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------|
| `crates/engine-core/src/scene_catalog.rs`, `project.rs`, `project_format.rs`                        | IDs/catalogs, schema-3 validation/shared settings and schema-2 read upgrade  |
| `crates/engine-core/src/game_session.rs`, `simulation_session.rs`                                   | Prepared scene replacement, simulation identity and resolved behaviors       |
| `crates/engine-core/src/editor/scene_documents.rs`, `history.rs`, `mod.rs`                          | Retained drafts, save baselines, navigation, cross-scene history and budgets |
| `crates/engine-core/src/command.rs`, `editor/factory.rs`, `engine.rs`, `events.rs`                  | Typed scene intent, snapshots, receipts and change publication               |
| `apps/editor-host/src-tauri/src/project_location.rs`, `editor.rs`, `commands/editor.rs`             | Root resolution, candidate replacement, persistence/reload policy and IPC    |
| `apps/editor/src/app/core/editor/editor.models.ts`, `editor.service.ts`                             | Protocol-2 frontend models and convenience methods                           |
| `compiler/src/lib.rs`, `apps/admin-dashboard/src-tauri/src/builder.rs`, `apps/headless/src/main.rs` | All-scene preparation/validation and runner loading                          |

Existing tests cover multi-scene history, project format, host lifecycle/path/persistence, frontend contract and
headless/compiler behavior. See the checkpoint in `docs-site/docs-state.json` for checks against the inspected working
tree and their limits; historical handoff test counts are not current-tree certification.
