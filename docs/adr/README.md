# Architecture decisions: reading guide and dependencies

This guide explains which part of the engine each ADR covers, which decisions to read first, and what still needs
evidence. ADR-0001 records accepted architectural direction with implementation planned. On 2026-10-29, core
state/lifecycle decisions in ADR-0004 and ADR-0005, and the native module/typed-state direction in ADR-0008, were
accepted. Their extended proposals and the other ADRs remain open. Acceptance and implementation/verification are
tracked separately; a document's existence does not establish a feature.

Date: 2026-10-27

Decision register updated: 2026-10-29.

## Resolved accepted cores

[Resolved architecture decisions](resolved/README.md) contains authoritative core pages for ADR-0004, ADR-0005 and
native/typed-state ADR-0008. Each separates decision status, implementation and verification. Original URLs retain
proposals/history. ADR-0001 remains accepted direction with substantial remaining UI work, not fully delivered.

## How the parts fit together

| Part                            | Responsibility                                                                                                               | Related decisions            |
|---------------------------------|------------------------------------------------------------------------------------------------------------------------------|------------------------------|
| Angular editor                  | Forms, subjects, graphs, selection views, layout, and user preferences. It sends editing intent and presents accepted state. | 0001, 0002, 0003, 0004       |
| Tauri host and bridge           | Typed communication, native window ownership, filesystem access, and worker coordination.                                    | 0001, 0003, 0012             |
| Engine authoring state          | Scene objects, components, references, validation, transactions, and undo history.                                           | 0001, 0002, 0004, 0009       |
| Runtime world                   | Temporary game state and scheduled execution of controls, movement, animation, and other supported systems.                  | 0004, 0005, 0007, 0008, 0010 |
| Content pipeline                | Source assets, stable references, imports, reusable prefabs, saved documents, and recovery.                                  | 0006, 0009, 0011             |
| Native presentation             | Vulkan surface, input routing, visibility, resize, GPU resource lifetime, and future multiple views.                         | 0005, 0010, 0012             |
| AI development and editor tools | Project-aware assistance, shared native tools, external MCP agents, retrieval, and model training/evaluation.                | 0001, 0003, 0004, 0008, 0014 |

This is the target division of responsibilities. The code already implements parts of it; each ADR identifies its
own inspected implementation and proposed additions. Animation evaluation, a general scripting runtime, linked
prefab overrides, and the proposed asset-ID pipeline must not be inferred from their editor controls.

## Decision register

The numbers establish a reading order, not a requirement to finish every preceding implementation before writing or
prototyping the next decision. The dependencies below concern architectural assumptions. Cross-references may also
describe integration between decisions that can be developed together.

| ADR                                                                                 | Question it answers                                                                                          | Read first                                            | Status                                         |
|-------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------|-------------------------------------------------------|------------------------------------------------|
| [0001 — Editor command pipeline](0001-editor-command-pipeline.md)                   | How does an edit become a validated, undoable backend change, and how are explicit builds coordinated?       | Entry point                                           | Accepted direction; implementation planned     |
| [0002 — Objects and components](0002-object-composition-and-components.md)          | What is an Object, what can be attached, and what is a Subject, Controller, Asset, or Preset?                | 0001                                                  | Proposed                                       |
| [0003 — Rust–TypeScript bridge](0003-rust-typescript-bridge.md)                     | Which types and values cross the boundary, in which representation, with what validation and delivery rules? | 0001, 0002                                            | Proposed                                       |
| [0004 — Authored, Play, and editor state](0004-authoring-play-and-editor-state.md)  | What is saved, what exists only during Play, and what belongs to the user's workspace?                       | 0001, 0002                                            | Core resolved; extensions proposed             |
| [0005 — Runtime lifecycle and scheduling](0005-runtime-lifecycle-and-scheduling.md) | When do systems run, how do components start/stop, and who is allowed to move an object?                     | 0002, 0004                                            | Core resolved; expansion proposed              |
| [0006 — Asset identity and import](0006-asset-identity-and-import-pipeline.md)      | How do references survive renaming, and how are imported resources rebuilt and released?                     | 0002, 0003                                            | Proposed                                       |
| [0007 — Graph assets and execution](0007-graph-assets-and-execution.md)             | How do editable nodes become validated animation programs or shader programs?                                | 0002, 0003, 0005, 0006                                | Proposed                                       |
| [0008 — Game modules and scripting](0008-game-modules-and-scripting.md)             | What can game code do, where does it run, and how are reloads and failures handled?                          | 0002, 0003, 0005, 0006                                | Native/typed core resolved; scripting proposed |
| [0009 — Prefabs and overrides](0009-prefabs-and-instance-overrides.md)              | How does reusable content relate to instances and their deliberate differences?                              | 0002, 0004, 0006                                      | Proposed                                       |
| [0010 — Input actions and possession](0010-input-actions-and-possession.md)         | How do devices, UI focus, players, and AI produce actions for the correct object?                            | 0004, 0005                                            | Proposed                                       |
| [0011 — Persistence and recovery](0011-persistence-autosave-and-recovery.md)        | What does Save mean across several files, and what survives a failure or external edit?                      | 0001, 0004; coordinate with 0006 and 0009             | Proposed                                       |
| [0012 — Native viewport composition](0012-native-viewport-composition.md)           | How does the game surface coexist with panes, dialogs, input, resizing, and native resources?                | 0001, 0003, 0004; coordinate with 0005 and 0010       | Proposed                                       |
| [0013 — Blender asset pipeline](0013-blender-asset-pipeline.md)                     | How does Blender export static models for validated reimport and builds without requiring Blender?           | 0006; coordinate with 0001, 0003, 0004, 0011          | Proposed                                       |
| [0014 — Open-weight assistant and MCP](0014-open-weight-assistant-and-mcp.md)       | How can one project-aware model implement logic and operate the editor while users connect their own agents? | 0001, 0003, 0004, 0008; coordinate with 0006 and 0011 | Proposed                                       |

## Suggested decision sequence

The accepted native progress/state/activation core is now present in the inspected tree: game-defined typed progress,
engine-owned candidates, aggregate world/progress commit, ordinary scene retention and prepared New Game. See
the [resolved pages](resolved/README.md) for current APIs, ownership and limits. Animation, component lifecycle,
scripting and broader UI remain proposals. Preserve shared-project/prepared-factory ownership, ChangeAccumulator and
document-cost caching.

Start by reviewing the object and bridge proposals together with **0004, 0005, and 0006**. These establish state
ownership, execution ownership, and resource identity. They provide the vocabulary and invariants for later work.

Next, review **0007 and 0008** before connecting graphs and scripts to runtime behavior. Review **0009 and 0011**
together before promising linked reusable content, Save All, or crash recovery. Review **0010 and 0012** while
connecting controls and the native viewport to the editor layout. Input and viewport behavior can be prototyped
alongside the content work as long as their ownership assumptions stay explicit.

Review **0013** with **0006** when connecting Blender authoring to the asset pipeline. Start with a tested static GLB
profile, one-click export, and explicit reimport; automatic refresh follows verified native resource replacement.

Review **0014** after the command, bridge, state, and game-module boundaries. Prove external MCP access to the live
editor and validated development tools before selecting and fine-tuning the built-in model. Model training does not
establish unsupported engine features or replace current project context.

The current implementation uses a small behavior set, one active simulation at a time, one viewport, and native modules.
The Player can prepare and replace registered scenes while retaining its `GameSession`; development-mode scene
transitions and restoration remain outside that feature. Later capabilities require their corresponding contracts
and acceptance evidence. Drafting broader decisions makes those extension points visible.

## A player example across the decisions

Consider a Guard prefab that the player can possess:

1. **0002** defines its object, transform, motor, animation, and audio components.
2. **0006** identifies its model, animation graph, and audio clips independently of their file locations.
3. **0009** explains which settings come from the Guard prefab and which this instance overrides.
4. **0004** keeps the saved spawn position separate from its position while playing.
5. **0010** routes a logical Move action to the possessed object and clears old control state when possession changes.
6. **0005** resolves movement and animation contributions in a defined order with one transform authority.
7. **0007 and 0008** define how supported graph and game-code behavior produce results within that schedule.
8. **0001 and 0003** carry authored edits and selected inspection data between the editor and the engine.
9. **0011 and 0012** govern saving/recovery and presentation in the native game pane.

This is an explanatory target workflow, not a claim that these gameplay features are implemented.

## What remains open

The individual ADRs distinguish a recommended starting policy from a choice that needs a prototype, measurement,
or product decision. In particular, a scripting language/runtime, native surface composition beyond the current
Windows child-window approach, cross-platform durability guarantees, and performance budgets need evidence. Do not
turn example limits, proposed DTO names, or scheduling sketches into compatibility promises accidentally.

When reviewing a proposal, record the alternatives considered, why the chosen option fits this engine, which other
ADRs it depends on, the migration impact, and acceptance criteria. Mark it Accepted only when the direction is agreed.
Track implementation and validation separately. A later change to an accepted direction should link a superseding
decision and preserve the earlier rationale.

## Inspection and verification scope

The drafting baseline is commit `fcb748b04853209b5e17e680c38c847c19f7f365` and the current staged, unstaged, and
untracked tree inspected on 2026-10-27. Source-specific notes in each ADR identify existing behavior and in-progress
work. The documentation checkpoint records publication checks and remaining limits; HEAD alone does not certify
the content or an uncommitted source change. The focused 2026-10-29 decision update additionally inspected
`b6ec4ba5a88c5ef66eff82fa61eb5747b940400d` and the staged/unstaged/untracked tree, including concurrent optimization
changes. It accepts only the identified core decisions and does not certify unrelated changes or runtime behavior.

These documents belong to the Engine Developer space. Documentation-site tests validate generated pages, navigation,
and links; runtime, compiler, filesystem failure, device, and performance acceptance require their own tests.
