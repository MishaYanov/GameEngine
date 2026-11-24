# Author scene objects and reusable Props

The **Hierarchy** shows the objects placed in the current scene. The **Inspector**
shows the selected object's transform and attached components. The **Assets** drawer
contains reusable project files, including Prop sources and imported models.

## Build a scene

1. Choose **Objects → Cube** or **Hierarchy → Create → Cube**. For a floor, create a
   **Plane**, scale it in the Inspector, and add a **Collider**.
2. Select an object and use **Add Component** for supported features. A mesh gives the
   object its appearance. A collider controls its physical shape. Use Scene roles
   in the Inspector to assign a camera or local player when the object supports it.
3. Create folders in Hierarchy and drag scene roots into them. Folders organize the
   Editor. To give an object a parent transform, use **Make Child Of…** or drag it
   onto a compatible object.
4. Open detailed settings from the attachment card. Animation opens the assigned
   clip. Animator and Controls Graph open their assigned assets. Audio, Behavior,
   and Particle Designer each have a focused authoring view.

Use the arrow keys to move focus in Hierarchy. In the workspace tab list,
use arrows to move focus and Enter or Space to select a tab. The Scene and Game
tabs share the native viewport. [Editor workspace](editor-workspace.md) ·
[Scene editing and commands](scene-editing.md)

## Reuse a group as a Prop

1. Select the objects and choose **Create Prop from Selection…** from Hierarchy.
   Review the pivot and any external references before saving the source.
2. Find the Prop in Assets. **Place Linked Prop…** keeps a link to a specific
   source version. **Place as Copy** makes an independent set of objects.
3. Open the asset in **Prop Workspace** to change its source. It has its own Undo,
   Redo, and Save Prop commands. Use **Show in Assets** to find the active source.
4. For a nested Prop, select the placement in the tree and choose **Open Nested
   Source**. Use the breadcrumb to return to the parent Prop and its placement.
   Use **Review Refresh…** when the parent should use a newer child version.

## Update a linked placement

Select the linked root or a member to inspect its source and overrides. Use
**Review Source Update…** to compare a newer source with the placement's saved
base and local edits. Resolve any conflicts before updating the placement.
**Revert Property** removes one local override. **Unpack** keeps the objects in
the scene and removes their links.

The Editor's **Play** command starts from the selected scene. The standalone
Player starts from the project's startup scene. Save your edits before exporting.
[Project lifecycle](project-lifecycle.md) · [Windows quickstart](windows-editor-quickstart.md)

## Change several objects together

Select several objects in Hierarchy and edit a local transform axis in the Inspector. A field shows
**Mixed** when its selected objects differ. Entering a value sets that axis on the selection.
Use Undo to restore all affected objects together.

Clear **Active in Play** to make the selected object and its descendants inactive when entering Play.
The Inspector identifies an inactive ancestor. To change a linked member's activation, open its Prop source.
You can switch off a whole linked placement at its root.

## Apply Play transforms to the scene

In the Game workspace, open Runtime Inspector and choose **Pause and Review Transforms…**.
Review the local transforms of existing objects and select the changes to keep. Applying them stops Play and
creates one Undo step in the authored scene. Choose **Keep Play Paused** to close the review without adopting changes.

## Attach a rotation routine

1. Select a mesh object, add a **Script** component and set **Script** to **Rotation Routine**.
2. Use **Routine JSON** to define how the object turns. This example sets its local yaw to 35 degrees on
   start and then turns it at 60 degrees per second.

```json
[{"during":"start","degrees":35},{"during":"fixed_update","degrees":60}]
```

3. Apply the component, then choose **Play**. Stop restores the authored rotation.

The routine runs against the object's local transform. Each entry accepts `during` and `degrees`. Keep
the routine within 256 UTF-8 bytes and six rules. A Character Controller owns its own object's transform.
Play and export validate the routine before running it.

## Create a graph for mesh color

Select a scene mesh and choose **Create Material Graph** in its component settings. Connect the **Color**
node to **Output** for a solid tint. Use **Multiply** and a scalar **Value** to scale the color.
Compile and apply the graph, then Save All. The saved RGB is used in Play and export. Adjust opacity
in the mesh material settings. The graph reports invalid connections before replacing the saved result.

## Remove an inherited object from one Prop placement

Select an inherited leaf in a linked placement and choose **Remove Object from This Instance** under
**Linked Prop**. Resolve that object's local property overrides first. Choose the linked root to
find the presence override and **Restore Object**. When an incoming source adds children to a
removed object, use **Use Source** to keep that updated branch.

## Add a point or directional light

1. Choose **Objects → Point Light** or **Objects → Directional Light**. You can also create one inside a
   folder from its Hierarchy **Create** menu. The Editor selects the new object.
2. Use the selected object's **Transform** to place a Point Light or aim a Directional Light. A Point Light
   follows the object's world position. A Directional Light shines along the object's local negative Z axis
   after applying its parent transforms.
3. In **Inspector → Light → Edit**, set the type, linear RGB color and intensity. **Range (metres)** controls
   a Point Light's reach. You can also attach **Light** to an existing object through **Add Component**.
   Choose **Apply Component** and **Save All**.

Light settings are saved with the object. This example is a Point Light component that fits inside a scene
object's `components` list.

```json
{
  "id": "light",
  "schema_version": 1,
  "enabled": true,
  "data": {
    "kind": "light",
    "settings": {"kind": "point", "color": [1.0, 0.75, 0.5], "intensity": 1.5, "range": 8.0}
  }
}
```

An older scene opens with lights mapped to objects under their original keys. When you save an edited scene,
the light component and object transform are written together. The renderer accepts up to 16 lights per scene.
The object state is defined in `crates/engine-core/src/components/light.rs`. Legacy light conversion lives in
`crates/engine-core/src/scene_data/lights.rs`. Runtime assembles the light pose in
`crates/runtime/src/scene/render.rs`.

## Drive a Character Controller with root motion

On an object with an enabled **Character Controller**, open its **Animator** binding and select a compatible
imported rig. Set **Root motion source node** to the GLB index of the rig ancestor that contains the clip's
planar translation. Apply the binding, then test the clip against an obstacle in Play.

The Animator supplies planar displacement to the motor each fixed step. The motor resolves collisions,
and the visual pose stays aligned with the accepted position. The motor continues to own jumping and gravity.
Clear the source node to let the usual movement input drive planar motion.
