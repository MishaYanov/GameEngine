# Particle Workbench

1. Copy this folder, then use **File → Open Project…** in the desktop Editor and open the copy's `project.json`.
2. Press **Play**. The Game view shows the orange Embers and blue Fountain effects. Press **Stop** to edit them.
3. Select **Fountain** in Hierarchy, then use **Edit Particles…** on its Particle Emitter component. Select **Sparks**
   to
   see its wider spread in the preview, or change the fields individually. Choose **Apply**, then **Close** to return to
   the scene.
4. Press **Play** again to see the change. Stop and use **Undo** to restore the previous emitter.
5. Use **Objects → Particle Emitter** to create another emitter and open its designer. Choose **Close**, then move the
   new object with its
   normal Transform controls. Use **Save All** to persist changes in your copy.

The effects use opaque camera-facing particles that shrink over their lifetime. They launch using the object's world
position and rotation, then move through world space. Transform scale leaves the launch cone and speed unchanged. The
designer preview runs independently of the scene.

To export the sample from the repository root, create a destination directory and pass its absolute path to the
compiler.

```powershell
$exportFolder = (New-Item -ItemType Directory -Path 'D:/Coding/Particle Workbench Export').FullName
cargo run --offline --locked -p compiler -- --export apps/headless/tests/fixtures/particle-workbench/project.json $exportFolder
```

Resources [Editor access and particles roadmap](../../../../../history/roadmaps/editor-access-and-particles-154-162.md).
