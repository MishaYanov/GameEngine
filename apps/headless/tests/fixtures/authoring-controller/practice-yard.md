# Build and export the Practice Yard

Open the finished yard first, then build the same assembly in an empty project. The finished scene has a motor-driven Player, a separate Game Camera, two linked Bell Gates, a marker-driven Practice Bell, and a Scene Sky.

## Open the reference yard

Run these commands from the repository root in PowerShell. Keep the copy for edits, playback, and repair exercises.

```powershell
$fixture = (Resolve-Path -LiteralPath 'apps/headless/tests/fixtures/authoring-controller/full').Path
$yard = Join-Path $env:TEMP ('practice-yard-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $yard | Out-Null
Copy-Item -Path (Join-Path $fixture '*') -Destination $yard -Recurse
$project = Join-Path $yard 'project.json'
$replay = Join-Path $yard 'route.replay.json'
.\dev.ps1 editor
```

Choose **File → Open Project…** and select `project.json` in the copy. Open **Game**, enter **Play**, and move with **Right Arrow** or the left stick. Press **J** or the bottom face button to jump. Press **F** or the left face button once to swing. Release the button before trying again. Look for the camera following the Player, movement animation, the outdoor Sky, and one bell sound shortly after Play starts. The Practice Bell has its own animation. In **Runtime Inspector**, refresh with the Player selected to inspect the Animator and Controls Graph. Stop Play and check that the authored positions return. [Project manifest](full/project.json) · [Scene](full/assets/scenes/full-practice-yard.json) · [Recorded route](full/route.replay.json)

## Build it in an empty scene

1. Choose **File → New Project…** and open the new project's scene. Import a GLB from **Assets** and drag it into **Scene** twice. Rename the two independent roots **Floor1** and **Floor2** in the Inspector. Choose **New Folder…** in **Hierarchy**, name it **Floor**, select both roots, and use **Move to Folder… → Floor**. Add a box Collider to each floor so it supports the Player. [Hierarchy and folder implementation](../../../../editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts)
2. Assemble a **Bell** with a movable visual object named **Bell Body**. Select the assembly, choose **Create Prop from Selection…**, and save it as `prefabs/bell.prefab.json`. Create a **Bell Gate** source at `prefabs/bell-gate.prefab.json` from the gate assembly. Keep the gate collider on its fixed support. Open the gate source with **Open Prop Workspace**, select the parent inside the gate, choose **Add Nested Prop…**, review the Bell source, then choose **Place Nested Prop** and **Save Prop**. [Bell source](full/assets/prefabs/bell.prefab.json) · [Bell Gate source](full/assets/prefabs/bell-gate.prefab.json) · [Nested Prop form](../../../../editor/src/app/modules/editor/workspace/prop-workspace/nested-prop-editor.html)
3. Place the gate twice with **Assets → Place Linked Prop…**. Open the gate source in **Prop**, change a child transform, and save. In the scene, use **Review Source Update…** on each placement to take the new version. Rename **Bell Body** in the first placed gate to **Left Bell**. Inspect **Linked Prop** on its root to see the name override. The second gate should still show **Bell Body**. Undo once and confirm the name override disappears, then redo, use **Save Scene**, and reopen from **File → Open Project…**. Check that the override and both nested placements come back. [Linked Prop Inspector](../../../../editor/src/app/modules/editor/workspace/prefabs/linked-prefab-inspector.html)
4. Choose **Hierarchy → Create → Player**. In the Inspector, choose **Set Local Player** and assign an Input Actions asset in the Player Controller. Use the reference [Input Actions](full/assets/controls/yard.input-actions.json), [Controls Graph](full/assets/controls/yard.controls.json), and [Animator](full/assets/animations/yard.animator.json) when reviewing the action IDs and states. From Player Controller choose **Open Controls Graph**. From Animator choose **Open Animator**. Rebind reusable animation channels to the current Player's visual children if you create a new scene. [Graph-wired yard](README.md#controller-and-animator-route)
5. Choose **Hierarchy → Create → Camera**, then **Edit Camera**. Set its rig to **Orbit** with **Player** as the target, apply the edit, and choose **Set as Game Camera**. Switch to **Game** to check framing. Use **Preview Selected Camera** when trying settings in Edit mode. [Camera Inspector](../../../../editor/src/app/modules/editor/workspace/components/scene-component-inspector.ts)
6. Add a **Practice Bell** with a visual child and a collider on the fixed parent. In **Animation**, key the visual before, during, and after its swing. Add one contact marker, save the clip, and attach a visual Animator to the Practice Bell with the visual child as the clip target. Add an **Audio Source** to the Practice Bell, assign a short WAV, choose **Spatial** and **On Event**, then set **Event source → Animation marker**. Pick the saved **Animation Clip** and **Timeline marker**. Keep **Loop** off and choose **Effects**. Use near and far distances of 0.7 m and 22 m for the reference yard. Scrub across the marker in Animation to check preview silence, then cross it in Game to check one bell onset. [Bell clip](full/assets/animations/practice-bell.clip.json) · [Bell Animator](full/assets/animations/practice-bell.animator.json) · [Bell WAV](full/assets/audio/practice-bell.wav) · [Windows stereo playback adapter](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutsetvolume)
7. Choose **Hierarchy → Create → Sky** and **Set as Scene Sky**. Use **Edit Sky** to set the zenith, horizon, and yaw. For a panorama, import a 2:1 PNG up to 2048 × 1024 from the Sky edit panel, then apply its assignment. Use **Game → Scene Information… → Scene Environment** to adjust ambient light and sky exposure independently. In **Prop**, switch **Preview environment** between **Neutral** and **Scene**, then close the pane and compare the Game view. [Sky settings](../../../../../crates/engine-core/src/components/sky.rs) · [Reference panorama](full/assets/textures/practice-yard-sky.png)

## Check, replay, and export

1. With the yard saved, open **Game → Scene Information… → Check Scenes**. Use an owner or workspace action to inspect its reference. On a disposable copy, temporarily move `assets/prefabs/bell.prefab.json` out of Assets. Check that the complete pinned Gate still opens, verification shows a warning, and the compiler can stage to a fresh destination. Restore the source. With a panorama assigned, temporarily move its PNG out of Assets. Check the Sky owner in the report and confirm export stops until the image is restored. [Scene verification](../../../../editor/src/app/modules/editor/workspace/scene-actions/scene-verification.html)
2. Run the recorded route and compare two data-only stages. Use a fresh copy of the finished yard if you changed object identities. Stage destinations must have room for a new project folder.

```powershell
cargo test -p engine-headless --offline --locked --test full_authoring_practice_yard
cargo run -p engine-headless --offline --locked -- --project $project --replay $replay
$stageA = (New-Item -ItemType Directory -Path (Join-Path $yard 'stage-a')).FullName
$stageB = (New-Item -ItemType Directory -Path (Join-Path $yard 'stage-b')).FullName
cargo run -p compiler --offline --locked -- --stage $project $stageA
cargo run -p compiler --offline --locked -- --stage $project $stageB

function Get-YardInventory($root) {
    Get-ChildItem -LiteralPath $root -File -Recurse | ForEach-Object {
        $relative = [IO.Path]::GetRelativePath($root, $_.FullName)
        $digest = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
        "$relative $digest"
    } | Sort-Object
}

Compare-Object (Get-YardInventory $stageA) (Get-YardInventory $stageB)
```

An empty comparison means the two stages contain the same paths and bytes. Try staging into `$stageA` again to check that the compiler rejects the occupied project folder. [Compiler staging](../../../../../compiler/src/export.rs) · [Headless replay CLI](../../../src/args.rs)

3. Choose **Export Game…** and use a new output directory. Keep `game.exe`, `project.json`, and `assets` together inside the finished project folder. Start the game from outside the package. Given the actual directory shown after export, run the following from the original project copy.

```powershell
$package = Read-Host 'Full path of the exported project folder'
$player = Join-Path $package 'game.exe'
Push-Location -LiteralPath $yard
try { & $player } finally { Pop-Location }
```

Repeat the keyboard and controller route in the standalone window. Check the camera, Player, nested gate, Sky, and one bell onset. For the Editor Game pass, compare the scene SHA-256 before Play and after Stop to check that playback kept the scene file intact. Capture Game and Player screenshots from the same camera view and record the headphone and controller results separately from the replay evidence. Ask a Unity-familiar tester to find **Hierarchy**, **Prop**, **Animation**, **Controls Graph**, and **Export Game…** and record where they need help. [Windows release rehearsal](../../../../../scripts/release-rehearsal-checklist.md)

## Resources

- [Controller and graph recipe](README.md) · [Graph replay comparison](../../authoring_graph_replay.rs) · [Full yard acceptance tests](../../full_authoring_practice_yard.rs)
- [Native viewport authoring workflow](../../../../../designs/editor-viewport/game-creation-workflows.md)
- [Cinder Oath project](../../../../../projects/cinder-oath/project.json) · [Native camera regression](../../../../game/tests/unit/tests_camera_native.rs)
