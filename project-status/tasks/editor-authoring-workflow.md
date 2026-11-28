# Editor authoring workflow

Status on 2026-11-26 · Windows authoring update verified in the local checkout on main at cc0e6b9, with existing staged
and working-tree changes preserved.

The expanded review began by 19:56 UTC on November 25. The requested minimum five-hour window runs through 00:56 UTC on
November 26. Build, test, temporary, and evidence files go to D:.

## Acceptance

Create a scene and object through visible commands. Keep viewport, hierarchy, and object Inspector selection in sync.
Make long options readable with the mouse and keyboard. Open an asset from Assets in Edit and browse its identity during
Play. Open a saved animation by identity, edit it, save it, and reopen that same clip. Exercise transform arrows, grid,
and Wireframe in the native renderer. Check those workflows in a packaged Editor at desktop and narrow sizes.

## References and decision

Unity exposes object creation and selection in Hierarchy and separates file browsing by scope and type. Its Animation
view opens clips against a scene object. The useful behaviors here are direct creation, a consistent selection target,
explicit search scope, and a clear difference between opening a saved clip and creating a new
one. [Unity Hierarchy](https://docs.unity3d.com/6000.0/Documentation/Manual/Hierarchy.html) · [Unity Project window](https://docs.unity3d.com/6000.0/Documentation/Manual/ProjectView.html) · [Unity Animation window](https://docs.unity3d.com/6000.0/Documentation/Manual/animeditor-UsingAnimationEditor.html)

Unreal connects viewport and Outliner selection to Details, exposes Actors through a placement workflow, and keeps a
file-oriented Content Drawer. Apply those relationships using this editor’s own controls and visual
design. [Unreal Editor](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-editor-interface) · [Unreal actor placement](https://dev.epicgames.com/documentation/en-us/unreal-engine/placing-actors-in-unreal-engine)

Godot keeps scene nodes, files, and inspected properties visible as separate concepts. Its dedicated Animation panel
works with a selected player or animation resource and an object
preview. [Godot editor](https://docs.godotengine.org/en/stable/getting_started/introduction/first_look_at_the_editor.html) · [Godot Inspector](https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html) · [Godot animation](https://docs.godotengine.org/en/stable/tutorials/animation/introduction.html)

Keep Angular and Tauri for this pass. Qt Widgets offers mature tree, docking, and menu controls. The straightforward Qt
port would still host a separate Vulkan native window. Qt documents that this window stacks above normal widgets. A
texture-composited Qt design is viable with a Vulkan render target and synchronization bridge. Neither path repairs the
current animation identity or editor command feedback on its own. First measure and fix the current workflows through
real desktop
tests. [Qt window containers](https://doc.qt.io/qt-6/qwidget.html#createWindowContainer) · [Qt Vulkan window](https://doc.qt.io/qt-6/qvulkanwindow.html) · [Qt texture composition](https://doc.qt.io/qt-6/qrhiwidget.html)

## Review passes

For each coherent change, record a comparison to the relevant engine workflow, inspect the implementation and error
boundaries, run focused regressions, then repeat the integrated desktop interaction at a second size or state. Record
the observed outputs and limits in each case.

## Checkpoint

D:/Builds/editor/Workspace Update is a checkpoint with the initial Assets toolbar, workspace actions, Material close
prompt, grid and Wireframe, Move picking, New Scene, and Animation preview changes. Before the expanded pass, 814
Angular tests, 68 focused native viewport tests, 3 New Scene native integration tests, and the Windows Vulkan smoke
passed. Current ownership covers Inspector, existing animation, and Assets through parallel agents. Main ownership
covers creation, shared menu and picker layout, reference comparison, desktop E2E, and release integration.

## Workflows checked against other editors

| Area                   | Reference behavior                                                                                                                                              | Game Engine acceptance                                                                                                                                                                                                                                                             |
|------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Hierarchy and creation | Unity Hierarchy and Unreal actor placement create an object inside the current scene. Godot adds scene nodes from its scene tree.                               | Create primitives, cameras, lights, or a controller through **Create Object**. The created object is selected and visible in the Hierarchy, its Scene viewport, and Inspector. Folder membership and parent attachment are explicit.                                               |
| Inspector              | Unity and Godot expose selected object properties and contained components. Unreal shows Details for the Outliner or viewport selection.                        | A persistent object heading precedes local Transform and Components. Component editing stays on its subject and returns focus to the action that opened it. A linked Prop source is opened in the Prop workspace through an explicit action.                                       |
| Assets                 | Unity searches inside a Project browser, Unreal Content Drawer keeps a browsable project collection, and Godot's FileSystem shows the project path.             | Search across the project or browse one folder. Common filters stay visible. More types shows optional catalog kinds. Saved authoring references and game audio are discoverable by path, including browsing while Play runs.                                                      |
| Viewport               | Unity positions objects with dedicated transform tools. Unreal highlights the active axis and offers Immersive Mode. Godot's distraction-free mode hides docks. | Move, Rotate, and Scale have distinguishable native controls. The exact visible handle is picked, captured drags show the active axis, and a single Undo restores a complete drag. Maximize Scene leaves view and transform tools reachable and restores the existing dock layout. |
| Animation              | Unity Animation view, Unreal Sequencer, and Godot's Animation panel lay out time horizontally and connect an animation to a visible subject.                    | Imported clips play against the chosen object. An authored asset opens in the Timeline with its actual name, path, and live preview. The full-width Timeline uses object names for tracks where available. Saving edits preserves the asset identity.                              |

[Unity positioning](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/positioning-game-objects) · [Unreal Content Browser](https://dev.epicgames.com/documentation/unreal-engine/content-browser-interface-in-unreal-engine?lang=en-US) · [Unreal Viewport controls](https://dev.epicgames.com/documentation/en-us/unreal-engine/viewport-controls-in-unreal-engine) · [Unreal Sequencer](https://dev.epicgames.com/documentation/en-us/unreal-engine/sequencer-cinematic-editor-unreal-engine?lang=en-US) · [Godot 3D editor](https://docs.godotengine.org/en/stable/tutorials/3d/introduction_to_3d.html)

## Qt comparison

Qt Widgets would provide built-in dock placement, floating panels, model/view trees, and layout persistence. Reusing the
current Vulkan child window in a Qt dock would retain native stacking and require explicit keyboard and pointer handoff.
Qt's render-to-texture route is different work. It would integrate a render target and Vulkan synchronization with the
UI compositor, then revalidate resize, modal dialogs, rendering, and input across platforms. The Qt Rust bridge can
carry typed signals and properties, but its standard bindings do not implement the whole Widgets API. Scene ownership,
asset identity, preflight, and undo would still need the same engine command model in either
shell. [Qt docking and layout state](https://doc.qt.io/qt-6/qmainwindow.html) · [Qt native child window constraints](https://doc.qt.io/qt-6/qwidget.html#createWindowContainer) · [Qt composition with QRhiWidget](https://doc.qt.io/qt-6/qrhiwidget.html) · [CXX-Qt bindings](https://github.com/KDAB/cxx-qt)

The packaged Angular/Tauri editor demonstrated native pointer editing, viewport focus and render resizing, and Vulkan
depth after the fixes. Qt did not offer a simpler route to the reported outcomes. A real Qt benchmark would need Qt
installed and a working engine surface integration. The comparison did not measure a separate Qt implementation.

## Findings from repeated review

1. Workflow and reference review connected selection, object creation, file browsing, and Timeline reopening to their
   authoritative native IDs.
2. Implementation review found three cross-layer failures. A background editor change read could remove the Create
   Object trigger and close its menu. Separate reservations allowed New Scene to register without opening. Saving a
   modern Scene could leave no legacy camera field, which made the Assets refresh parse the scene as a prefab. The
   revised flows preserve menu ownership, reserve creation with activation, and respect registered scene paths during
   asset migration.
3. Narrow desktop and accessibility review covered 640, 760, 1080, and 1440 CSS pixels, keyboard and mouse input, draft
   and file confirmation, and original per-project layout restoration.
4. Native verification used the packaged WebView and the Vulkan screenshot layer. The depth fixture confirmed fully
   occluded geometry changes zero output pixels and grid lines preserve mesh interior pixels. Native mouse messages
   verified selection readback, one-axis drag, and one-step Undo. This injection tests the Editor window procedure
   without using physical mouse movement.

Evidence and commands are under apps/editor-e2e and the D-drive target/editor-workspace-qa folders. The scope is the
Windows editor checkout and packaged sample projects.

The complete Windows editor host suite passed 157 tests with one ignored. The most recent full Angular suite passed 848
tests after the picker, Inspector, Timeline, Assets and Prop changes. The D-only installer build generated both a
portable Editor and its optional-dashboard NSIS setup. The packaged export E2E verifies the player and Cinder Oath
soundtrack by hash. A separate packaged run opened a disposable copy of the standalone Hollow Maze and found all six
game-used files, including its sky panorama in Play.

A first sustained trial recorded 63 complete two-scene cycles before one scene was accepted while native presentation
stalled. The attempt had insufficient runtime status detail to prove the cause. A compressed native recheck performed
112 exact presentations without reproducing it. The maintained sustained test now records native viewport and browser
state if it stalls, verifies the exact rendered scene ID and revision, and adds repeated animation preview. The final
long run remains pending.

A second New Scene edge case was reproduced when opening the dialog before the Vulkan viewport reports its generation.
The startup dialog hides the renderer. Creation will instead use a direct trusted native workspace-scope read before the
generation-guarded write in the same editor action reservation.

### Packaged verification

The Windows candidate at D:/Builds/editor/Authoring Release 2 includes native startup scope for both New Scene and
primitive creation. The primitive defect reproduced as a visible Create menu with Plane, Cube, Sphere, and Capsule
missing on two of twelve earlier starts. Removing the renderer dependency retained native revision and
project-generation checks. The final portable package completed four separate 10-case desktop test passes. The final
Angular suite passed 855 tests and the native Editor host passed 157 with one ignored. The package script also confirmed
that the production Angular entry is embedded in the executable and verified the packaged resources by hash.

The depth screenshots validate surface occlusion while preserving visible edges. A staged Windows minimize/restore trial
accepted an alternate authored scene while the native viewport was suspended, then drew that exact scene revision after
restore. The 455×294 backbuffer resized to 695×469, with foreground focus unchanged. Neither trial attributes the
earlier isolated presentation timeout.

### Completed sustained verification

The five-hour review window included 3:27:33 of uninterrupted automated Windows testing on the final portable package.
It completed 1038 authoring cycles and 2076 exact native scene presentations at four window sizes, with no recurrence of
the earlier timeout. The median switch took 531 ms and the 95th percentile took 553 ms in the Cinder Oath fixture. The
sampled Editor process working set reached 168.9 MiB, excluding WebView and GPU processes. The raw log is in
target/editor-workspace-qa/authoring-final-sustained/authoring-soak-n5024V/cycles.jsonl.

The final x64 standalone export started its real player and reported `GAME_STARTUP_SMOKE_OK mode=player`. The
detached-viewport case created a Cube and opened a new scene without the native surface. Focused Win32 checks acquired
and committed each visible Move, Rotate, and Scale handle, with one Undo per drag. Synthetic off-foreground Rotate and
Scale input verified commit and Undo. Live drag preview for those two tools was outside that validation. The NSIS
installer was built and its matching portable package was exercised. The installer was left unapplied to the user's
Windows profile.

### Editor Back and right-click creation · 2026-11-26

The Windows Editor on main (base cc0e6b9) now keeps navigation history for Assets folders, workspace views, and scene
switches in the open project. Scene changes retain draft review, and Assets has its own Back, Forward, and Up controls.
Project Setup replaces the desktop route on entry to the editor. The packaged cold creation check reproduced the setup
redirect on Authoring Release 2 and verified the replacement route.

Short native Scene right-clicks open Workspace actions at the pointer. Primitive creation uses the snapped work-plane
point. Right-drag still controls the camera. Empty workspace chrome also offers Create Object, New Scene, and Build
Game. A startup trace captured an enabled Empty Object choice dropped while the Input Actions catalog was loading.
Catalog lists now use the guarded read path shared by Animator and Controls Graph, while edits remain on the native
command queue.

The Angular suite passed 864 tests. The native Windows host suite passed 160 tests with one ignored. The final portable
package in D:/Builds/editor/Editor Navigation and Context 2 passed all 15 desktop cases and 28 repeated cold-start
object-creation trials. With the Input Actions response deliberately held, the previous package disabled Empty Object
and the final package created it while the response was still pending. The final installer was built, and desktop
verification used its matching portable Editor. Logs are under target/editor-workspace-qa in
navigation-fixed-desktop.log, cold-menu-fixed.log, and held-input-catalog-before.log and held-input-catalog-after.log.
