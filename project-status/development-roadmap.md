# Game Engine development roadmap

The next product target is a Windows authoring workflow where someone can create a prop in Blender with the Agent,
review and import it in the Editor, build a small playable scene around it, and export a game that runs on another
machine. The [current project state](README.md) records the starting point and its verification.

The cycles start from the reviewed Windows installation. They cover engine behavior and authoring workflows. Editor UI
design is a separate effort.

## Cycle order

Each cycle should end with a demonstrable user workflow and checks that the next contributor can repeat. Split a cycle
into smaller pull requests where useful. Keep acceptance tied to one recorded revision and fixture so results can be
compared.

| Cycle                                  | What the author can do                                                               | Depends on                            |
|----------------------------------------|--------------------------------------------------------------------------------------|---------------------------------------|
| 1. Authoring regression checks         | Verify open, edit, save, recovery, and Play on a reproducible revision               | Current checkout                      |
| 2. Playable scene flow                 | Author and verify a complete multi-scene route with a reusable Timeline              | 1                                     |
| 3. Reliable streamed worlds            | Cross region boundaries smoothly and recover from load failures in both native hosts | 2                                     |
| 4. Agent-assisted Blender assets       | Generate one editable Blender prop and adopt its GLB in the Editor                   | Existing static GLB export and import |
| 5. Reviewed scene composition          | Preview and apply an Agent-authored scene layout as one undoable edit                | 4                                     |
| 6. Agent-assisted game behavior        | Wire a complete key, door, transition, and finish loop from approved building blocks | 2 and 5                               |
| 7. Agent-assisted export qualification | Reproduce the completed workflow and run its standalone export independently         | 3 and 6                               |

Cycles 2–4 describe the next three product features. Their milestone numbers belong to this roadmap, and the referenced
engine milestones keep their existing numbers. Blender recipe work can run alongside the authoring checks and Scene
Flow.

## Cycle 1 · Qualify the current authoring changes

**Outcome.** A contributor can start the Editor from one known revision, open an existing project or create a blank one,
and recover from a failed save without losing edits.

- Review and land the pending viewport, startup and mouse input, asset reuse, and scene-flow changes as coherent slices.
  Review index-only paths and untracked sources with their owners. Record which slices are present in the candidate
  revision.
- Exercise Scene and Prop inspection together with native input. Cover Lit, Unlit, Wireframe occlusion, selected
  vertices, mixed DPI, overlay focus, pointer look, Escape and IME. Bound the cost of dense vertex inspection and
  surface a clear state if a mesh exceeds the wireframe budget. Verify the Game view keeps the authored appearance.
- Keep native workspace commands as the owner of saves and recovery. Show which documents saved, which conflicted, and
  what can be retried. Record save and recovery latency on a representative large project before changing lock
  boundaries.
- Add a short, maintained Windows smoke path for the workflow. Start with repeatable fixtures and automate steps that
  can produce reliable evidence. Record the actions that still need a visible check.

**Acceptance.** The standard suite and production Editor build pass at a recorded revision. A visible run demonstrates
Open Existing and New Project, Save All with an external edit, draft recovery after restart, viewport mode and input
isolation, and Play/Stop with unchanged authoring history for view controls. An external MCP client can use the existing
consent flow, then loses access after revocation. Record Windows version, GPU, DPI, and
outcomes. [Testing](../docs/testing.md) · [Native verification baseline](../history/reviews/persistence-viewport-blender-mcp.md) · [Viewport handoff](tasks/viewport-shading.md)

## Cycle 2 · Finish one complete playable scene flow

**Outcome.** An author can build a small route from an entrance scene through a playable room to an exit, with an
opening or arrival Timeline and a reliable restart.

Start from the local Scene Flow panel, typed routes and Timelines, and the existing hidden three-scene fixture. The
current test run covers the core contracts. The next evidence is a visible authoring and playback run on a revision that
contains those
changes. [Project status](README.md#verification) · [Engine milestones 234–239](../designs/scene-flow-and-streaming.md#milestones)

| Milestone                                         | Demo                                                                                                                                 | Complete when                                                                                                                                                                                                                                                                 |
|---------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **2.1 Author and reopen a route** · 234, 239      | Use the existing Scene Flow panel to connect an entrance sensor to a named arrival in a second scene.                                | Save and reopen preserve the start scene, trigger, destination, and arrival binding. Undo and Redo work through existing commands. Validation identifies the route and the missing binding when the destination changes.                                                      |
| **2.2 Finish transition recovery** · 235–236      | Trigger the route while preload is pending. Cancel once, then retry after restoring an unavailable destination.                      | The source remains playable with its progress intact. The author can see whether the transition is waiting, failed, or ready to retry. Duplicate triggers produce one activation, and a completion from a previous run is rejected. Exercise both Editor Play and the Player. |
| **2.3 Return control after a Timeline** · 237–238 | Reuse the same Timeline with two sets of scene-local bindings for a reveal camera and a cue.                                         | Natural finish and Skip settle completion once. Cancel releases temporary camera and input ownership without applying completion effects. A missing binding produces a useful error.                                                                                          |
| **2.4 Play and export the same route** · 238–239  | Take the versioned fixture from entrance to playable room to exit in Editor Play, an exported Player, and a bounded headless replay. | Record the named arrival, cue order, completion, and restart results. The visible exported Player runs from a separate working directory and reaches the same game outcomes as the saved source.                                                                              |

Use the existing [scene-route test commands](../docs/testing.md#scene-routes-timelines-and-streamed-regions)
and [handoff](tasks/scene-flow-and-streaming.md) to record the revision and results for each milestone.

## Cycle 3 · Qualify streaming in a visible Release build

**Outcome.** An author can run a representative level through region boundaries without persistent hitches or climbing
memory, and retry a failed region while the game stays usable.

The local implementation already has a three-region fixture, Player retry, an Editor region overview, and hidden-window
measurements from a 49-region grid. Use those as the starting
evidence. [Engine milestones 241–250](../designs/scene-flow-and-streaming.md#seamless-open-world-milestones) · [Runtime streaming](../crates/runtime/src/world_streaming.rs)

| Milestone                                                   | Demo                                                                                                                   | Complete when                                                                                                                                                                                                                                                                                                                    |
|-------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **3.1 Cross and reverse at a seam** · 241–243               | Walk, sprint, and reverse through a three-region route in Editor Play and the Player.                                  | Incoming collision is ready before crossing. The player and camera keep their current state. Reversing course keeps the needed neighbor resident, and a late load leaves a usable source region.                                                                                                                                 |
| **3.2 Retry an individual region in both hosts** · 243, 246 | Temporarily make one region unavailable, restore its asset, and retry in place.                                        | The Editor reports which region failed and offers retry, matching the existing Player recovery path. A stale request after Stop, restart, or project switch is rejected. The active world remains playable throughout.                                                                                                           |
| **3.3 Preserve state in an independent export** · 244–245   | Open a door or collect an item, leave the region until it unloads, then return in an exported copy.                    | Declared changes survive the revisit, and the compiler packages the required region data and shared assets. If the fixture supports Save and Continue, verify its declared state after opening it in a new Player process.                                                                                                       |
| **3.4 Qualify visible Release performance** · 233, 240, 247 | Repeat the 49-region fixture and at least 30 matched A↔B switches per direction in each of three visible Release runs. | The fixture stays within eight resident regions and its agreed frame and memory budgets. Report p95 and worst presentation gap, CPU update time, upload and fence time, and peak and settled memory under matched conditions. Repeated visits settle, and the evidence records whether the existing pruning policy stays in use. |

Use a named fixture, build, and GPU for measurements. Use the visible results to decide whether the existing LOD,
navigation, or precision work needs
extending. [Local timing baseline](../docs/performance-baseline.md#scene-switch-and-streaming-measurements)

## Cycle 4 · Generate and reimport a Blender prop

**Outcome.** From a short request such as “make a stone door frame with these dimensions,” the Agent can prepare a
versioned recipe. Blender produces an editable `.blend` and a compatible static GLB, which the author previews and
imports through the Editor.

The existing Blender add-on exports a static GLB with stable mesh IDs, and the native Editor already supports import and
reimport. Start with one stone doorway recipe and prove it in the current
Editor. [Blender exporter](../integrations/blender/game_engine_export/blender_addon.py) · [Native asset adoption](../apps/editor-host/src-tauri/src/commands/editor/asset_authoring.rs)

| Milestone                                      | Demo                                                                                                                                                       | Complete when                                                                                                                                                                                                                                            |
|------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **4.1 Define one versioned prop recipe**       | Use measurements, a pivot, and a base-color input to describe a stone doorway.                                                                             | The recipe specifies units, stable mesh IDs, supported material inputs, and output limits. Tests reject invalid dimensions and unsupported content. The existing static fixture renders in the native Editor.                                            |
| **4.2 Generate a Blender candidate**           | A reviewed Blender runner builds the doorway from its recipe in a host-assigned job directory.                                                             | The job produces an editable `.blend`, a validated self-contained GLB, and a receipt with recipe and tool versions, sizes, and hashes. The runner reports timeout, cancellation, and validation errors. Only validated outputs are offered for adoption. |
| **4.3 Review, adopt, and regenerate the prop** | Review the candidate in the Editor, place it twice, change its geometry, and reimport it.                                                                  | Adoption checks the reviewed candidate against the current project and catalog. Both placements keep mesh identity, references, and transforms. A failed reimport preserves the previous usable asset.                                                   |
| **4.4 Demonstrate the MCP handoff**            | A compatible external client reads authorized Editor context, calls the bounded Blender recipe tool, and returns the candidate for approval in the Editor. | The Editor's inspection lease stays read-only. Invalid and cancelled calls leave the project catalog intact. After approved adoption, the exported Player renders the asset from its packaged files.                                                     |

Keep the proposed Blender tools scoped to recipe generation and its result. Scene edits and the built-in Agent
dispatcher follow in Cycle
5. [Static GLB reference](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/docs/blender_docs/scene_gltf2.rst#file-format-variations) · [MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

## Cycle 5 · Apply a reviewed scene layout

**Outcome.** The Agent proposes a layout using approved assets. The author sees the exact changes in the Editor and can
apply or discard them.

- Introduce a versioned, typed scene proposal for a narrow initial set of commands such as place, parent, transform and
  assign existing components. Resolve stable asset and entity IDs and preview the resulting hierarchy and transforms
  using the native validation path.
- Apply an approved proposal for one scene through existing commands and history. Bind approval to the actual plan,
  workspace generation, base scene revision, target IDs, and asset fingerprints. Revalidate those values at apply time.
  Keep asset adoption and project-manifest changes independently reviewable.
- Add a bounded tool dispatcher to the built-in Agent and extend its drawer to show proposed changes, rejected
  references, and approval status. Route project changes through validated native commands. Tool calls and results carry
  a correlation ID through the existing logger.

**Acceptance.** Place and parent a door frame, key pedestal and light, then apply them as one Undo step with Redo
support. A changed scene, replaced asset, switched project, expired approval, or invalid plan causes zero authored
mutations. Check exact intended output after Save and Reopen. Test the same validators through the Editor and the
proposal tool
adapter. [Editor commands](../crates/engine-core/src/command.rs) · [Agent and MCP ADR](../history/adr/0014-open-weight-assistant-and-mcp.md) · [MCP tools and confirmation](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)

## Cycle 6 · Build and verify a complete game loop with the Agent

**Outcome.** The Agent helps an author build a small playable level from a prompt. The reference level uses a generated
key and door, a locked gate, a successful pickup, a route to an exit scene, and a clean restart.

- Suggest from existing behaviors, components, inputs and Timeline cues. The Editor explains the roles, target
  references, and initial state for each behavior before adoption. Treat a new gameplay primitive as engine work with
  its own design and tests when the catalog cannot express a request.
- Make scene and project route edits reviewable in stages. For example, apply the door behavior and object references in
  one scene edit before proposing the destination route in the project manifest. Run dependency checks before Save and
  Export.
- Store a small canonical fixture and a replay with explicit outcomes. Include failure cases in the feature, so the
  player learns why the door is locked and the author can find a missing target.

**Acceptance.** Starting from the recipe and plan, a second contributor can reproduce the playable project. The door
stays locked before pickup, unlocks after pickup, and only the expected route activates. The exit and restart reset the
intended state. Verify Save/Reopen, a headless replay, native Editor Play, and the exported Player. Compare observable
behavior and the defined replay checkpoints across source and staged
builds. [Behavior catalog](../crates/engine-core/src/components/native_behavior.rs) · [Game modules](../crates/gameplay-catalog/src/lib.rs) · [Source and staged replay tests](../apps/headless/tests/authoring_graph_replay.rs)

## Cycle 7 · Qualify the Agent-assisted export

**Outcome.** An author can reproduce the Agent–Blender workflow in the existing Editor installation and share a
self-contained playable export.

- Reproduce the reference project through generation, reviewed import, scene and behavior approval, Save and Reopen.
  Check that the packaged worker can load its resources and that the compiler captures all adopted asset dependencies.
- Use the existing Player qualification, checksum, and signoff process for the new export. Test affected packaging when
  a change adds shipped resources. Route reproducible error and timing evidence into the Dashboard through the existing
  logger.
- Run the reference game from a different working directory in a separate Windows environment. Keep the prepared
  project, packager, and Player on the same behavior and asset contract.

**Acceptance.** Reproduce the reference route in Editor Play, export it, and complete it in the standalone Player with
only its packaged contents available. Verify input, audio, presentation, asset identity, and restart. Record the exact
candidate, fixture, and hardware
evidence. [Compiler export](../compiler/src/export.rs) · [Qualification playbook](../scripts/release-qualification-acceptance.md) · [Windows Player job](../ci/jenkins/Jenkinsfile.windows-release)

## Cycle handoffs

The project orchestrator records the revision, fixture, owner, demo outcome, checks, and next executable step
in [task handoffs](tasks/). Use the documentation, verification, performance, and defensive security roles for
independent checks. Route code structure to the existing Angular, Rust, and Tauri owners. When a material design
decision needs the author's judgment, use the [design interview](../.agents/skills/grill-me/SKILL.md) and record the
decision with the task.
