# Blender characters, rig controls, and script assets

## Confirmed design · 2026-11-26

The user approved character import and reimport first, followed by runtime foot IK, then Rust script assets and
Editor-configurable projectiles and particle effects. The implementation is local to `main` at base `cc0e6b9`. The index
and working tree already contain unrelated changes.

1. Add a skinned Blender character from Assets to a scene and bind Idle and Walk. The Character export profile provides
   persistent joint and clip IDs. Re-export with renamed or reordered content and verify that existing bindings still
   refer to the intended joints and clips. A missing referenced ID needs a repair path and the last accepted asset
   should remain usable. Include compatible conventional GLBs when they share placement and decoding. Require review
   where identity lacks evidence.
2. Configure foot IK on the imported character in the Editor. Save, reopen, and verify the foot adjusts on a step in
   both Editor Play and the exported Player. Repeat after a compatible Blender re-export.
3. Attach a `.rs` asset to a Scene object or Prop, edit its exposed settings, and compile changes for the next Play
   session. Each attachment owns runtime state. Expose typed lifecycle, game clock, inputs, Animator, particle, and
   impact APIs through the existing simulation command boundary. Prototype Rust-to-WebAssembly with a compatible
   dependency set. Show source diagnostics and package the compiled artifact for the Player.
4. Create a Projectile Prop in the Editor. Set movement, collision layers, lifetime, trail, and impact effect in the
   Inspector and bind launch to an Input Action. A Rust script can select the impact effect. Verify in Editor Play and
   the standalone export that the trail follows the projectile and an impact burst survives removal of the projectile.
   Reuse bounded particle storage and measure repeated effects before changing allocation strategy.

## Implementation and verification

- Blender now exports the Character profile from File → Export. Blender 4.0.2 and 5.3.0 passed the generated rename and
  clip reorder fixtures and the existing static exporter fixture. The shared decoder retains stable symbols in Animation
  and Animator bindings. Native placement passed 13 host asset tests, including Undo and Redo, Save and Reopen, and
  Editor Play for a standard GLB. The Animation Inspector prompts for explicit reassignment of a changed standard GLB.
- The Animator Inspector offers Foot IK for one ankle and its two deform parents. The Blender leg fixture reached 0.20 m
  over flat terrain and 0.38 m over the step, with matching EditorSession and compiler-staged GameSession pose traces
  after re-export. Ten focused Rust and 54 Angular checks passed. Visible Windows presentation is unverified for Foot
  IK.
- PNG sprite import uses the existing asset catalog. The Designer previews the selected texture. The runtime reuses its
  bounded storage and dynamic mesh, groups draw ranges by sprite, and renders depth-tested alpha cutouts. A hidden
  Vulkan capture confirmed image orientation and transparency. A Projectile Launcher binds an Input Action to a reusable
  Prop, with an independent impact burst. Focused projectile, particle, lifecycle, and component editor checks passed in
  isolated checks while the Rust script runtime was being integrated.
- Rust script source authoring and Inspector attachment use the catalog. Editor Play builds a separate WebAssembly
  artifact with a fixed toolchain command. Export stages the compiled artifact. Both real Rust package tests passed.
  They cover distinct attachment state, typed references and settings, simulation time, source-line failure reporting,
  staged playback, and a scripted impact burst that outlives its Projectile. The authoring compiler accepts a standalone
  `.rs` file using the script SDK and Rust standard library. Stateful scripts explicitly serialize fields with
  `read_state` and `write_state`.

The real Character reimport test passed against both Blender versions. A generated export of the Foot IK project ran the
current native `game.exe` and returned `GAME_STARTUP_SMOKE_OK mode=player` after Vulkan presentation. The Foot IK step
trace comes from the EditorSession and compiler-staged GameSession comparison. The Windows Vulkan sprite capture passed
with two texture batches and confirmed the PNG row orientation and cutout.

`cargo check --offline --locked -p app -p game -p engine-headless` passed. The combined workspace checks passed 1,457
Rust tests with 31 ignored, 872 Angular Editor tests, 16 Dashboard tests, 161 Python tests, and the Rust documentation
command. Four existing socket tests needed approved loopback access. A headless regression caught changed Animator
dependency diagnostic wording. The original wording was restored and the headless and workspace cases passed on the
rerun.

The final portable Editor is at `target/character-features-release-final/Editor`. Both packaged desktop cases passed. A
new Rust asset was created and attached in the Inspector, wrote a 0.62 radian runtime rotation, preserved the authored
scene after Stop, and exported with its frozen WebAssembly module. The exported executable launched from a separate
working directory and returned `GAME_STARTUP_SMOKE_OK mode=player`. The other desktop case placed the Blender GLB
through Assets, opened the character preview, and verified the Idle and Walk clip names. Evidence is in
`target/character-features-final-desktop`.

Desktop verification caught a final export inventory rule that rejected declared `.rs` and `.wasm` files after staging.
The final compiler checks passed with the inventory fix and the Rust package regression now completes and verifies a
staged export as well as checking its simulation. Foot IK over the step was verified through matching pose traces. The
separate native startup smoke verified presentation. Projectile impact and trail behavior were verified through staged
simulation and the shared Vulkan sprite capture.

- [Blender pipeline decision](../../history/adr/0013-blender-asset-pipeline.md)
- [Game modules and scripting](../../history/adr/0008-game-modules-and-scripting.md)
- [Windows authoring handoff](editor-authoring-workflow.md)
