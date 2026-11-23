# Editor authoring workflows, Milestones 122–133

Open a desktop project with a scene, some nested asset folders, and an authored animation clip. The exit exercises
also use editable Animator and Controls Graph assets. Code paths below are relative to the repository root.

## 122. Route title-bar Close to the active editor

Open the editor and make a scene change. Press the title-bar **X**. The native host intercepts the close request
and sends it to the active editor when its listener is registered. If the editor cannot receive that review, the
window remains open and gives you a way to retry or use **File → Exit**.

In `apps/editor-host/src-tauri/src/lib.rs`, keep interception in the window event handler. In
`apps/editor-host/src-tauri/src/commands/project.rs`, target the requesting WebView and send its registered owner.
Use the existing trusted-window check in `apps/editor-host/src-tauri/src/commands/editor.rs`. Keep the bounded
registration and owner-matched cleanup in `apps/editor-host/src-tauri/src/state.rs`.
A full page load in the main WebView clears the old close-review registration.

```rust
.emit_to(
    EventTarget::webview_window(window.label()),
    WINDOW_CLOSE_REQUESTED,
    owner,
)
```

Run the owner replacement and native fallback tests in
`apps/editor-host/src-tauri/tests/unit/commands/tests_project.rs`. They cover a late unregister from an old
listener, invalid owner tokens, and an editor route with no ready listener. A physical title-bar click remains a
desktop acceptance check.

Resources [Native close review](editor-authoring-polish.md#119-review-native-window-close-requests) ·
[Angular and host contract](project-lifecycle.md#angular-and-host-contract).

## 123. Review graph drafts before exiting

Make an unsaved change in Animator or Controls Graph. Request Close using the title bar and choose to keep
editing. Your draft stays open. Repeat through **File → Exit** and check that it offers the same draft review.
If you proceed, the native host can still ask about remaining scene, project, Timeline, and Prop source changes.
While the native exit request is pending, workspace controls are temporarily inactive. Choosing Keep Editing
or encountering an exit error restores those controls.

In `apps/editor/src/app/modules/editor/desktop-close-review.ts`, install the listener before registering its owner
and match the incoming event to that owner. In `apps/editor/src/app/modules/editor/editor.ts`, send both entry
points through `exitEditor()`. Reserve one review, verify the workspace generation after it resolves, and use
the existing exit command.

```ts
if (!disposed && payload === owner) requestReview();
```

In `apps/editor/src/app/modules/editor/editor.spec.ts`, check that repeated close requests share the same review,
cancelling leaves the native exit command untouched, and **File → Exit** remains available if registration fails.

Resources [Exit confirmation and revision checks](editor-authoring-polish.md#120-recheck-the-work-covered-by-exit-confirmation) ·
[Authoring data and runtime state](../designs/editor-viewport/game-creation-workflows.md#4-authoring-data-and-runtime-state).

## 124. Review drafts before leaving the Editor route

Change an Animator or Controls Graph asset and choose **File → New Project**. Cancel the review to stay in
the same Editor view with the draft open. Complete the review to reach project setup. Going back or visiting
another route within the app also uses this review. Opening setup leaves the desktop window and native workspace
open.

In `apps/editor/src/app/app.routes.shared.ts`, guard departures from the Editor route. In
`apps/editor/src/app/modules/editor/editor.ts`, route departure, Open Project, and Exit through
`withReviewedWorkspace()`. Share the review reservation and workspace checks. The route guard approves
navigation without calling the native exit command.

```ts
canDeactivate: [(component: Editor) => component.reviewRouteDeparture()],
```

In `apps/editor/src/app/app.routes.spec.ts`, cancel navigation and check that the same component stays mounted.
Then approve and check that setup opens without an exit invocation.

Resources [Open a desktop project](editor-workspace.md#open-a-desktop-project) ·
[Documents, switching, and history](project-lifecycle.md#documents-switching-and-history).

## 125. Filter Assets by kind

Open **Assets** and choose **Models**, **Props**, **Scenes**, or **Audio Clips** from **Asset kind**. Enter a name
or path in **Search Assets** to search that kind across the project. Choose **Animators** or **Controls Graphs**
on hosts that support them to open the matching registry group. Clear the search to return to the folder you
were browsing.

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`, keep the typed kind options and
filter the catalog entries before building the file view. In
`apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.html`, bind the options to the kind
selector and open the active graph registry.

```ts
const filter = this.assetFilter();
return filter === 'all' ? this.entries() : this.entries().filter(entry => entry.kind === filter);
```

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.spec.ts`, check that filtering can
hide the selected file, a name search still finds files in other folders, and resetting the filter preserves the
browsing folder.

Resources [Search Assets by name or project path](editor-authoring-polish.md#113-search-assets-by-name-or-project-path) ·
[Asset identity and source editing](../designs/editor-viewport/game-creation-workflows.md#73-asset-identity-and-source-editing).

## 126. Jump back to an ancestor folder

Open nested folders under **Assets**, such as **props → outdoor → stairs**. Click **props** in the breadcrumb or
Tab to it and press Enter. The panel returns directly to **props** and focus lands on its current-folder label.
Choose **Assets** to return to the root. **Up** still moves one level.

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`, derive each crumb from the
current folder and reset browsing state when the workspace generation changes. In
`apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.html`, use native buttons for ancestors
and `aria-current="location"` for the current crumb. Use
`apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.scss` to wrap long paths in narrow panes.

```ts
if (this.editor.workspaceGeneration() === generation && !this.searching() && this.folder() === path) {
    this.folderLocation()?.nativeElement.focus();
}
```

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.spec.ts`, verify the direct jump,
focus, cleared file selection, and reset when a different project opens with the same folders.

Resources [Asset browser workflow](editor-authoring-polish.md#113-search-assets-by-name-or-project-path) ·
[Place and arrange reusable assets](../designs/editor-viewport/game-creation-workflows.md#7-workflow-c-place-and-arrange-reusable-assets).

## 127. Reveal a search result in its folder

Choose **Models** and search for an asset with a name that also appears in another folder. Select the intended
result and click **Show in Folder** in its details or context menu. Assets clears the query, keeps **Models**,
opens the correct folder, and selects and focuses the exact row. Scene, Prop, and Audio Clip results use the
same file action. Animator and Controls Graph results keep their own registry groups.

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`, resolve the result against the
visible catalog, then reuse the folder reveal and rendered-row focus used for linked Props. Bind the details
button and row reference in `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.html`.

```ts
const visible = this.folderContents().files.find(entry => this.sameFile(entry, file));
if (visible) this.revealAsset(visible);
```

In `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.spec.ts`, exercise duplicate display
names and the Scene context action. Check that an action from an older search leaves the current query intact
and that revealing a Scene file leaves the active scene unchanged.

Resources [Asset identity and source editing](../designs/editor-viewport/game-creation-workflows.md#73-asset-identity-and-source-editing) ·
[Project documents and switching](project-lifecycle.md#documents-switching-and-history).

## 128. Collapse the Hierarchy or expand a branch

Expand two unrelated branches in **Hierarchy**. Click **Collapse All**. Open the context menu on an object or
folder and choose **Expand Branch** to reveal its nested contents while the other branch stays folded. If the
focused row disappears when you collapse everything, focus moves to a visible ancestor. The scene selection
stays intact.

In `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts`, keep expansion
as local Hierarchy state. Add the toolbar action in
`apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.html`. Use
`apps/editor/src/app/modules/editor/workspace/scene-entities-panel/hierarchy-tree.ts` to traverse one branch,
keeping result projection inside the existing virtual tree.

```ts
this.collapsed.update(collapsed => expandHierarchyBranch(this.forest(), collapsed, key));
this.focus(key);
```

In `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts`, check that a
branch expansion leaves its sibling branches collapsed. Try searching while branches are folded and clearing the
query to recover the pre-search arrangement.

Resources [Hierarchy folder search](editor-authoring-polish.md#114-reveal-the-contents-of-matching-hierarchy-folders) ·
[Large-tree behavior](../designs/editor-viewport/game-creation-workflows.md#83-large-tree-behavior).

## 129. Select an object's parent or children

Open an object row's context menu in **Hierarchy**. Choose **Selection → Select Parent** to jump to its parent
object. Choose **Selection → Select Children** to select its direct children and reveal the first child as the
primary selection. The commands depend on the clicked object, including when another object is selected.

In `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts`, resolve parents
and children from the existing Hierarchy forest. Reveal the primary object and emit the selection through the
existing scene selection output.

```ts
this.cancelParenting();
this.revealEntity(primary);
this.selectionAnchor = primary;
this.selectionSetRequested.emit(keys);
```

In `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts`, check an
object with multiple children and a grandchild. Only its immediate children enter the selection, its parent can
be selected from a child row, and the scene's parenting remains intact.

Resources [Selection, snapping, hierarchy, and history](scene-editing.md#selection-snapping-hierarchy-and-history) ·
[Hierarchy organization](../designs/editor-viewport/game-creation-workflows.md#8-workflow-d-organize-a-scene-without-losing-spatial-intent).

## 130. Reset a specific local transform channel

Select an object with non-default Position, Rotation, and Scale in **Inspector**. Click **Reset Rotation** to
set its local angle to zero while preserving position and scale. **Reset Position** returns the local position
to zero, **Reset Scale** returns the scale to one on each axis, and **Reset Transform** restores all three.
Use **Undo** to restore the prior authored values.

In `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html`, emit the requested
channel through `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.ts` and forward it from
`apps/editor/src/app/modules/editor/workspace/editor-workspace.html`. In
`apps/editor/src/app/modules/editor/workspace/scene-editing.service.ts`, settle pending Inspector edits, read
the accepted transform for the same selected object, and send one local transform command.

```ts
const transform = field === 'all' ? IDENTITY_LOCAL_TRANSFORM :
    {...entity.transform, [field]: IDENTITY_LOCAL_TRANSFORM[field]};
await this.execute(CommandBuilder.transforms([{key, transform}], 'local'), generation);
```

In `apps/editor/src/app/modules/editor/workspace/scene-editing.service.spec.ts`, check that each reset updates
only the requested channel, a queued edit on another channel is preserved, and resetting an already-default
channel creates no additional history entry.

Resources [Reset a local transform](editor-authoring-polish.md#115-reset-a-local-transform-from-the-inspector) ·
[Authoritative document API](scene-editing.md#authoritative-document-api).

## 131. Preview a clip at a different speed

Open an authored clip in **Timeline**. Choose **0.25×**, **0.5×**, **1×**, **1.5×**, or **2×** under
**Playback speed**, then press **Play Clip**. Choose **Stop Clip** before changing the rate.
The speed applies to preview time, while saved key times, duration, and undo history retain their authored values.

In `apps/editor/src/app/modules/editor/workspace/animation/timeline.html`, bind the rate selector and disable
it during playback. In `apps/editor/src/app/modules/editor/workspace/animation/timeline-edit.service.ts`,
capture the validated rate at preview start and scale the elapsed real time before applying the loop or end bounds.

```ts
const next = from + Math.floor((now - started) * rate);
```

In `apps/editor/src/app/modules/editor/workspace/animation/timeline.spec.ts`, run the fake-clock test and check
that a half-speed preview advances by half the elapsed milliseconds without sending an authoring edit.

Resources [Timeline draft review](editor-authoring-polish.md#117-review-unsaved-changes-before-closing-a-timeline) ·
[Authoring data and runtime state](../designs/editor-viewport/game-creation-workflows.md#4-authoring-data-and-runtime-state).

## 132. Jump to the previous or next key or bookmark

In **Timeline**, choose a track with several keys and add a bookmark. Use **Previous Key** or **Next Key** in
the transport, or focus the Timeline lane and press Page Up or Page Down. Add Shift to jump to the adjacent
bookmark. The Bookmark section also has **Previous Bookmark** and **Next Bookmark** buttons. Select a key at an
off-grid time to check that the playhead lands on its exact timestamp even with **Snap to 10 ms** enabled.

In `apps/editor/src/app/modules/editor/workspace/animation/timeline.ts`, resolve adjacent key times on the
selected track and bookmark times across the clip. Stop preview and seek to the stored time.
In `apps/editor/src/app/modules/editor/workspace/animation/timeline-edit.service.ts`, keep snapping as the
normal seek behavior and allow these exact navigation actions to bypass it. Bind the buttons and describe
the shortcuts in `apps/editor/src/app/modules/editor/workspace/animation/timeline.html`.

```ts
const key = this.adjacentKeys()[direction];
if (!key) return false;
this.selectKey(key);
```

In `apps/editor/src/app/modules/editor/workspace/animation/timeline.spec.ts`, verify the off-grid seek, the
Shift shortcut, disabled buttons at the ends, and an untouched draft history.

Resources [Timeline authoring and close review](editor-authoring-polish.md#117-review-unsaved-changes-before-closing-a-timeline) ·
[Keyboard dispatch priorities](../designs/editor-viewport/scene-view-interactions.md#8-keyboard-dispatch-priorities).

## 133. Read clip timing on a zoomable ruler

Open a Timeline with keys and bookmarks. The ruler shows the start, midpoint, and end in milliseconds.
Scrub the playhead and compare it to the key and bookmark markers. Increase **Zoom** and scroll horizontally
inside the Timeline lane to see the same relative positions at a larger scale. Jump to an off-screen key or
bookmark to bring the playhead into view while keyboard focus stays where it was.

In `apps/editor/src/app/modules/editor/workspace/animation/timeline.ts`, normalize timestamps as a percentage
of clip duration. After a key or bookmark seek, reveal the playhead by adjusting the lane's horizontal scroll.
Bind the marker positions in `apps/editor/src/app/modules/editor/workspace/animation/timeline.html`. In
`apps/editor/src/app/modules/editor/workspace/animation/timeline.scss`, contain the wider ruler inside the
scroll lane. Keep keyboard operation on the existing Timeline controls.

```ts
return Math.min(100, Math.max(0, tick / Math.max(1, this.timeline.duration()) * 100));
```

`apps/editor/src/app/modules/editor/workspace/animation/timeline.spec.ts` checks marker alignment across zoom
levels and verifies focus and outer scroll when the lane reveals an off-screen playhead. A headless Edge
layout sample passed all four combinations of 340 px and 650 px panes at 1× and 4×. The ruler sample kept
horizontal overflow inside the lane at 4× and maintained each marker's relative position.

Resources [Editor workflow test commands](testing.md#editor-authoring-workflows-122133) ·
[Workspace sizing and dock changes](../designs/editor-viewport/scene-view-interactions.md#34-dpi-resizing-and-dock-changes).
