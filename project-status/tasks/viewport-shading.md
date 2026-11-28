# Editor viewport shading and vertices

**Status** · Local implementation, automated checks passed.

**Checkout** · Local main working tree based on cc0e6b9, checked 2026-11-25.

Scene and Prop Workspace have independent Lit, Unlit, and visible-edge Wireframe modes. Show Vertices adds markers to
selected mesh objects in each mode. The editor renderer keeps the per-frame display preference separate from authored
materials, scene history, and game rendering.

## Verification

- Bridge viewport contract tests passed.
- Native editor viewport unit tests passed.
- Prop preview token and history test passed.
- Renderer unit tests passed.
- Hidden native Vulkan renderer smoke passed.
- Angular Scene and Prop viewport tests passed, including an earlier object selection completing after a nested
  placement was selected.

**Next step** · In the desktop editor, overlap two meshes, select the rear one, and check that its hidden edges and
vertex markers disappear in all three modes. Switch between Scene, Prop Workspace, and Game to check view isolation and
confirm scene dirty state stays clear.
