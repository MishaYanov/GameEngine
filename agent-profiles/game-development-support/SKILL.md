---
name: game-development-support
description: Help design, implement, teach, or review game-authoring workflows and playable milestones in the Game Engine project, including scene editing, viewport interaction, gameplay configuration, and editor-to-player parity.
---

# Game Development Support

Help the user finish a small game while improving the engine capabilities that game actually needs.
Prioritize responsive authoring, clear game/editor state boundaries, and a complete playable loop.

## Find the relevant context

Resolve paths from the Game Engine repository root, not from this installed skill directory.
Read the applicable `AGENTS.md` instructions and inspect the current working tree before making changes.
For viewport and authoring work, read the relevant portions of `designs/editor-viewport/`.
Use `designs/editor-viewport/game-creation-workflows.md` for the reference game, milestone dependencies, and current-versus-proposed distinctions.
Use the interaction and runtime/rendering chapters when changing viewport tools or ownership.
Treat those documents as design guidance; recheck implementation claims against current source.

Useful source entry points:

- `apps/editor/src/app/core/editor/editor.models.ts`: typed editor contracts.
- `apps/editor/src/app/modules/editor/workspace/`: authoring UI and edit lifetimes.
- `apps/editor-host/src-tauri/src/`: workspace, native viewport, and host commands.
- `crates/engine-core/src/`: authored data, commands, simulation, sessions, and progress foundations.
- `apps/game/project.json` and `apps/game/src/progress.rs`: sample configuration and application-specific progress.
- `compiler/src/lib.rs`: validated creation, staging, and player export.

## Work from the user's goal

Identify the next action the user needs to perform in the editor or game.
Inspect existing commands, components, and runtime behavior before proposing another framework.
Choose a compact feature that closes an observable creation or play loop.
Keep game-specific rules in the game/module layer unless the engine needs a reusable capability.
Scale the number of steps to the feature; do not split trivial edits merely to fill a step count.
If the user wants teaching, explain the location, purpose, and visible result of each action.
If implementation is authorized, complete the functional slice and relevant verification.

## Preserve the important boundaries

Scene documents contain authored initial data.
Editor navigation, selection, and tools are editor-session state.
Live gesture previews are temporary until accepted or canceled.
Play creates runtime state; it must not silently overwrite authoring data.
Typed game progress may outlive a runtime scene according to the game's policy.
The standalone player consumes the authored game without editor-only grid or selection overlays.
Treat editor camera movement and explicit game-camera authoring as different actions.

## Make recommendations concrete

Use the reference-game pattern: build a route, configure a player, complete an objective, transition, finish, restart, and export.
Reuse shared behavior definitions and typed command paths when they fit.
Before claiming editor/player parity, verify that both select the same gameplay module factory and progress/restart policy.
Label unsupported features as future work, including full prefab overrides, arbitrary script reload, or unfinished graph editors.
For lag, locate the responsible path before suggesting a UI rewrite or new serialization format.
Separate input-to-present delay, command acceptance, asset loading, simulation time, and compilation.
Treat performance budgets as targets until measured on a named fixture and machine.
Use current primary sources for external engine or library behavior that needs verification.

## Deliver and report

Lead with the user-visible result and explain the practical implementation boundary.
Distinguish source inspection, automated checks, native interaction, and measured performance.
Preserve unrelated staged, unstaged, and untracked work.
Do not stage, commit, or publish unless the user explicitly authorizes that action.
Update maintained documentation only when requested, following the repository checkpoint and site rules.
Do not trigger named maintenance agents as a routine consequence of using this skill.
Use collaboration only when it fits the current authorized task and the user's current preferences.
Leave the next feature grounded in observed game-creation friction rather than a broad infrastructure wish list.
