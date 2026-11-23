# Editor authoring polish, Milestones 110–121

If you use Unity, start with **Hierarchy** for scene objects, **Inspector** for local properties, and **Assets** for
project files. Open a Prop to inspect it in isolation. Use **Animation** for clips and **Animator** for the state graph.
**Controls Graph** is the editor's separate graph for input and gameplay controls. The following exercises start from
an open project. Paths are relative to the repository root.

The next milestones cover close and project navigation, Assets browsing, Hierarchy actions, Inspector resets,
and Timeline navigation. [Editor authoring workflows 122–133](editor-authoring-workflows-122-133.md).

## 110. Activate workspace tabs with Enter or Space

Tab to the workspace tabs. Use Left and Right to move between available tabs and Home or End to jump to either end.
Press Enter or Space to open the focused pane. Clicking a tab opens it directly.

In `apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.ts`, keep track of the focused tab
separately from the active pane. In `apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.html`,
bind focus, keydown, and the roving `tabindex`.

```ts
if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    if (!event.repeat) this.selectView(currentKind);
    return;
}
```

Check that moving focus from Prop to Animation leaves Prop open until activation. When focus leaves the tab list,
the selected tab becomes its next Tab stop. See `apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.spec.ts`.

Resources [W3C manual-activation tabs](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/examples/tabs-manual/) and
[editor workspace controls](editor-workspace.md#connected-controls-and-current-limits).

## 111. Return to Animation after Stop

Open Animation and preview an imported clip. Click **Play**, then **Stop**. Play selects Game. Stop reopens
Animation if it is still available and starts a fresh imported preview for the current target.

In `apps/editor/src/app/modules/editor/workspace/scene-viewport/workspace-view.service.ts`, capture the previously
selected pane when the accepted snapshot enters Play. Keep the preview lifetime in
`apps/editor/src/app/modules/editor/workspace/animation/animation-workspace.service.ts`.

```ts
if (playing && !this.wasPlaying) {
    this.returnFromPlay = previous === 'game-preview' ? 'scene-viewport' : previous;
    this.activeView.set('game-preview');
}
```

Check that Play releases the previous preview token and Stop obtains a new token using the accepted revision.
The test drives the real view service in
`apps/editor/src/app/modules/editor/workspace/animation/animation-workspace.spec.ts`.

Resources [Scene, Game, and Play design](../designs/editor-viewport/scene-view-interactions.md#32-scene-game-and-play-transitions)
and [Unity Animation view](https://docs.unity.com/en-us/engine/6000.0/manual/animation-section/animation-mecanim/animation-clips/animation-editor-guide/animeditor-using-animation-editor).

## 112. Fall back to an available workspace tab

Open a Prop, then open another project. The old Prop tab disappears and Scene becomes active. An unsupported
Animator or Controls Graph pane follows the same fallback rule. Game is the fallback for an unavailable selection
while Play is active.

Derive pane availability from the snapshot, Play mode, Prop ownership, and native capabilities in
`apps/editor/src/app/modules/editor/workspace/scene-viewport/workspace-view.service.ts`. Bind the same list to
`apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.ts`. The panel in
`apps/editor/src/app/modules/editor/workspace/editor-workspace.html` obtains its label ID from the selected tab.

```ts
readonly activeViewKind = computed(() => {
    const selected = this.activeView();
    if (this.availableViews().includes(selected)) return selected;
    return this.editor.playing() ? 'game-preview' : 'scene-viewport';
});
```

Verify that one tab is selected, its ID matches the panel's `aria-labelledby`, and removing a focused tab moves
focus to a valid tab when the browser has dropped it to the page body. See
`apps/editor/src/app/modules/editor/workspace/scene-viewport/workspace-view.service.spec.ts` and
`apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.spec.ts`.

Resources [W3C tabs pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/) and [project replacement](project-lifecycle.md).

## 113. Search Assets by name or project path

In **Assets**, open a folder and type a filename or project-relative path in **Search Assets**. Results include
matches from other folders. Use **Audio Clips** to limit results by kind. Clear the search to return to the folder
you were browsing.

Normalize and filter the project catalog in
`apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`. Render the search field, result paths,
and empty state in `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.html`. Keep the layout
rules in `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.scss`.

```ts
if (query) {
    if (searchable(entry.name).includes(query) || searchable(entry.path).includes(query)) files.push(entry);
    continue;
}
```

Check that a mixed-case query using Windows backslashes finds a project path, matched Animator and Controls
Graph assets can still be opened, and a new project starts with an empty query. See
`apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.spec.ts`.

Resources [Unity Project window](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/project-view)
and [engine asset pipeline](graphics-and-assets.md).

## 114. Reveal the contents of matching Hierarchy folders

Create a **Floor** folder containing **Floor 1**, **Floor 2**, and **Trim**, with an unrelated folder beside it.
Collapse Floor and search for its folder name. Its contents include Trim while searching. Clearing the query
restores the original expansion state.

In `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/hierarchy-tree.ts`, keep ancestors of
direct matches and traverse descendants of matching folders using the existing presentation tree. Keep the
test in `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts`.

```ts
while (folderBranches.length) {
    const key = folderBranches.pop()!;
    if (visitedBranches.has(key)) continue;
    visitedBranches.add(key);
    matches.add(key);
    for (const child of rowChildren.get(key) ?? []) folderBranches.push(child.key);
}
```

Check that ancestors remain visible for an object match, a folder match includes its descendants, a keyboard
user can reach the rows, and a large result still uses the virtual row window.

Resources [W3C tree view pattern](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/) and
[Hierarchy interaction design](../designs/editor-viewport/scene-view-interactions.md#30-hierarchy-drag-and-drop).

## 115. Reset a local transform from the Inspector

Select a child object with edited position, rotation, and scale. In **Inspector → Transform**, choose
**Reset Transform**. The child's local position and rotation become zero and its local scale becomes one.
Undo restores the previous values.

Add the button and output in `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html`
and `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.ts`. Route the output through
`apps/editor/src/app/modules/editor/workspace/editor-workspace.html` to `resetTransform()` in
`apps/editor/src/app/modules/editor/workspace/scene-editing.service.ts`.

```ts
await this.execute(CommandBuilder.transforms([{key, transform}], 'local'), generation);
```

The service finishes the pending field edit, rechecks the project, scene, and selected object, and then sends one
transform command if anything needs to change. Verify that Undo restores the object, its sibling is untouched,
and resetting an already default transform does not create another history entry. See
`apps/editor/src/app/modules/editor/workspace/scene-editing.service.spec.ts`.

Resources [Unity Inspector window](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-inspector)
and [scene editing contract](scene-editing.md).

## 116. Frame a node or connection in a graph

In Animator or Controls Graph, select a node and press **F** with focus in the graph, or click
**Frame Selection**. A selected connection frames its two endpoint nodes. **Fit graph** still frames the whole graph.

In `apps/editor/src/app/shared/components/graph/graph.ts`, share the camera fitting calculation between the
whole graph and the selected cards. Place the new action in
`apps/editor/src/app/shared/components/graph/graph.html`.

```ts
return this.frameCards(this.cards().filter(card => {
    const id = card.node().id;
    return id === selected || id === edge?.from.node || id === edge?.to.node;
}));
```

Verify that the camera moves for both selection kinds and that the authored node positions and graph history
are preserved. See `apps/editor/src/app/shared/components/graph/graph.spec.ts`.

Resources [editor camera design](../designs/editor-viewport/scene-view-interactions.md#9-editor-camera-model)
and [editor workspace controls](editor-workspace.md#connected-controls-and-current-limits).

## 117. Review unsaved changes before closing a Timeline

Open **Animation → Authored Clips**, open a clip in the Timeline, edit a key or marker, and choose
**Close Timeline**. The dialog offers **Save and Close Timeline**, **Discard and Close Timeline**, and
**Keep Editing**. A clean Timeline closes directly.

Handle the request and the save/close results in
`apps/editor/src/app/modules/editor/workspace/animation/timeline.ts`. Render the dialog in
`apps/editor/src/app/modules/editor/workspace/animation/timeline.html`. Wire the child’s close and save events in
`apps/editor/src/app/modules/editor/workspace/animation/authored-clip-editor.html` and
`apps/editor/src/app/modules/editor/workspace/animation/authored-clip-editor.ts`.

```ts
if (await this.timeline.close(choice === 'discard')) {
    this.closing.set(false);
    this.closed.emit();
}
```

Check that Keep Editing retains the draft, a failed save keeps the Timeline open with feedback, and the parent
returns to the clip library only after the host accepts the close. See
`apps/editor/src/app/modules/editor/workspace/animation/timeline.spec.ts`.

Resources [Unity Animation view](https://docs.unity.com/en-us/engine/6000.0/manual/animation-section/animation-mecanim/animation-clips/animation-editor-guide/animeditor-using-animation-editor)
and [project persistence](project-lifecycle.md).

## 118. Include Timeline drafts in File → Exit review

Save the Scene and project, edit a Timeline marker, then choose **File → Exit**. The native dialog identifies
the unsaved Animation Clip Timeline and offers **Discard and exit** or **Keep editing**. Keep editing retains
the draft so you can return to Animation and save the clip.

In `apps/editor-host/src-tauri/src/commands/project.rs`, include the Timeline store alongside the scene/project
session in the native exit review. The File menu also runs the Angular review in
`apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` for Prop, Animator, and Controls Graph drafts.

```rust
let scene_dirty = session.is_some_and(|session| session.is_dirty());
let timeline_dirty = workspace.is_some_and(EditorWorkspace::has_dirty_clip_timelines);
```

Check that a dirty Timeline with a clean scene still produces a warning, and that a clean session and clean
Timeline produce no warning. See `apps/editor-host/src-tauri/tests/unit/commands/tests_project.rs`.

Resources [project lifecycle and saved baselines](project-lifecycle.md) and
[Timeline close review](#117-review-unsaved-changes-before-closing-a-timeline).

## 119. Review native window-close requests

With an unsaved scene, project, Timeline, or Prop source, request close using the title-bar **X**. The native review should
show the affected work and allow you to keep editing. After approval, the host schedules closure through the
viewport's shutdown path.

In `apps/editor-host/src-tauri/src/lib.rs`, intercept the native event. Route it to the shared review and
protect it against concurrent exit requests in `apps/editor-host/src-tauri/src/commands/project.rs` and
`apps/editor-host/src-tauri/src/state.rs`. Keep deferred viewport shutdown in
`apps/editor-host/src-tauri/src/viewport.rs`.

```rust
tauri::WindowEvent::CloseRequested { api, .. } => {
    api.prevent_close();
    commands::project::request_window_close(window.clone());
}
```

At the end of this batch, the title-bar route reviewed native scene, project, Timeline, and Prop source state,
and File → Exit also ran the Angular draft review. The next batch shares the Angular review with the title bar.
[Current close workflow](editor-authoring-workflows-122-133.md#123-review-graph-drafts-before-exiting).

The Rust tests in
`apps/editor-host/src-tauri/tests/unit/commands/tests_project.rs` exercise the revision source and
single-review gate. A physical click on the Windows title-bar button and its dialog flow remain unverified.

Resources [Tauri WindowEvent](https://docs.rs/tauri/latest/tauri/enum.WindowEvent.html#variant.CloseRequested)
and [native viewport implementation](viewport-implementation.md).

## 120. Recheck the work covered by exit confirmation

Capture the workspace generation, session revision, and Timeline and Prop draft revisions before showing the native
confirmation. Verify that capture under the lock before retiring the workspace. If the project is replaced or
the work changes during the prompt, the editor stays open and asks you to try Exit again.

In `apps/editor-host/src-tauri/src/commands/project.rs`, capture and compare `ExitReview`. In
`apps/editor-host/src-tauri/src/animation_preview/timeline.rs`, collect draft IDs and revisions in stable order.
Each Timeline draft has its own revision, independent of the scene revision.

```rust
let mut revisions: Vec<_> = self
    .0
    .iter()
    .map(|(id, draft)| (id.clone(), draft.history.revision()))
    .collect();
revisions.sort_unstable();
```

Run the exit review tests in `apps/editor-host/src-tauri/tests/unit/commands/tests_project.rs`. They cover edits
and Undo on an already dirty Timeline, Prop edits, edits to a scene, and a replacement project with the same
session revision. `apps/editor-host/src-tauri/tests/unit/animation_preview.rs` covers revision advancement on clip save.
After accepting Exit, `apps/editor-host/src-tauri/src/state.rs` records that shutdown has started. Both native
Open commands check that state in `apps/editor-host/src-tauri/src/commands/editor/documents.rs` before installing
a prepared project.

Resources [project replacement and revision checks](project-lifecycle.md) and
[testing and validation](testing.md#editor-authoring-polish).

## 121. Check the authoring workflow as a whole

In `apps/editor/src/app/modules/editor/workspace/editor-workspace.editing.spec.ts`, open a Prop from Assets,
arrow to Animation, and activate Animation with Enter. Start and stop Play, switch back to Prop, then replace
the project. Verify the visible pane, focus recovery, and the label relation at each step.

```ts
expect(panel.getAttribute('aria-labelledby')).toBe(selected?.id);
expect(selected?.getAttribute('aria-controls')).toBe(panel.id);
```

For layout, review `apps/editor/src/app/modules/editor/workspace/editor-workspace.scss`,
`apps/editor/src/app/modules/editor/workspace/animator/animator-workspace.scss`, and
`apps/editor/src/app/modules/editor/workspace/control-graph/control-graph-workspace.scss`. A representative
fixture compiled those styles and used headless Edge to measure nine cases. With maximized drawers, Scene and
Game retained 328 px of center space in a 1000 px workspace. At 680 px the center stacked above the drawers.
Animator and Controls Graph placed the outline below the canvas at 390 px and beside it at 650 px. All nine
fixtures had zero horizontal overflow. These are CSS layout measurements on representative markup. The Angular
test uses a mocked native bridge and viewport component.

Run the integration test and production release check from `apps/editor`.

```powershell
npm.cmd test -- --watch=false --include=src/app/modules/editor/workspace/editor-workspace.editing.spec.ts
npm.cmd run check:production
```

Resources [CSS container queries](https://www.w3.org/TR/css-contain-3/),
[editor verification commands](testing.md#editor-authoring-polish), and
[workspace sizing and DPI design](../designs/editor-viewport/scene-view-interactions.md#34-dpi-resizing-and-dock-changes).
