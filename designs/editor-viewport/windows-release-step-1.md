# Windows release refactors, Step 1

The goal is a Windows Editor that creates an editable project and exports a playable folder from its own installation.
The same step aligns what happens to open editing drafts when a user switches scenes, starts Play, exports, or leaves
the project.

## 1A. Separate packaging from Player compilation

**Problem.** `compiler/src/export.rs` currently builds the Player with Cargo as part of completing a staged export. The
Windows Editor needs a way to complete that step using its copy of the Player.

Keep the existing prepare, stage, and publish checks. Add a compiler entry point that accepts the executable from the
trusted native host. The developer entry point can build the executable explicitly, then use the same publication path.

```rust
let staged = stage_prepared_project(&prepared, destination)?;
complete_staged_project_with_player(staged, &player_path)
```

**Verify.** The supplied Player is copied to the validated new export. A changed staged document, an invalid source
executable, and an occupied destination fail without replacing existing files. The developer compiler command still
works from the source checkout.

Resources [Build and Player behavior](../../docs/project-lifecycle.md#build-and-playerheadless-behavior) ·
`compiler/src/export.rs`.

## 1B. Ship the Player and sample with the Windows Editor

**Problem.** An installation cannot depend on paths from the build machine. Build the Windows Editor and Player from the
same locked checkout. The release wrapper at `apps/editor-host/build-windows-release.ps1` stages the executable and the
Cinder Oath sample and supplies them to the Tauri bundle. Production IPC resolves these paths from the Editor’s
resources. The native host selects the executable, and the frontend supplies only the destination. Both Editor export
commands use the same source policy.

```text
Editor resources/
  release/player/game.exe
  release/samples/cinder-oath/project.json
  release/samples/cinder-oath/assets/
```

**Verify.** Build the Windows artifact, then create and export a project using the installed or portable Editor from an
unrelated directory. The exported Player starts from its own folder. A missing bundled resource produces a clear error.
Record separately any installer or native interaction checks the available environment supports.

Resources [Native Editor development and packaging](../../docs/apps/editor-host/README.md#development-and-packaging) ·
`scripts/release-qualification-acceptance.md`.

## 1C. Give Blank and Cinder Oath distinct project setup choices

**Problem.** New Project currently copies Cinder Oath even when an author wants a new world. Put a **Blank project**
choice first in `apps/editor/src/app/modules/project-setup/`. It creates a generic Main scene and a Game Camera using
the engine's normal project and scene formats. **Cinder Oath sample** creates an editable copy in the chosen folder. Use
the same path validation and project-open confirmation for both choices.

```ts
type ProjectTemplate = 'blank' | 'cinder_oath';
```

**Verify.** Blank opens, saves, reopens and stages without source assets. The sample opens as a new copy and retains its
gameplay module. Choosing either option leaves an existing destination and the original sample untouched.

Resources [Project documents and switching](../../docs/project-lifecycle.md#documents-switching-and-history) ·
`compiler/src/scaffold.rs`.

## 1D. Review workspace drafts at workflow boundaries

**Problem.** Scene switching, Play, export and project exit currently use different combinations of draft and revision
checks. Add the review in `apps/editor/src/app/modules/editor/workspace/workspace-draft-review.ts` and call it before
those actions. Reuse each editor’s existing draft review and preserve the scene-switch choice to retain authored edits
for later. Keep the previously selected project and scene in scope while waiting for a decision. Read the latest
revision after a successful save.

```ts
const target = drafts.capture();
await drafts.run(target, async () => {
    const snapshot = editor.snapshot();
    if (!snapshot || !drafts.current(target)) return;
    await editor.prepareBuild(destination, target.generation, snapshot.revision);
});
```

**Verify.** Retaining a draft cancels a pending action. Saving uses the resulting revision. A change to the source scene
or workspace prevents the action from running in a different target. Clean navigation and authoring command order keep
working.

Resources [State ownership](runtime-and-rendering.md#3-state-model) · [Review and recovery](scene-view-interactions.md#37-failure-and-recovery-matrix).

## Step 1 walkthrough

Install or extract the Windows Editor, create a Blank project in a separate folder, open its Main scene, start and stop
Play, use Save All, reopen the project, and export it. Start the exported Player from an unrelated working directory.
Repeat the draft checks before exporting a copy of Cinder Oath. Keep the exact artifact, revision, and environment with
the test results.
