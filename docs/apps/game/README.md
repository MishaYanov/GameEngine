# Native game demo

Source: `apps/game`. Cargo package: `game`. The application owns its winit window, scene, asset cache, and low-level Vulkan drawing resources. These scene/model APIs currently live in the game application rather than shared `runtime`.

## Run

From the repository root after [setup](../../usage.md):

```powershell
cargo run -p game
.\dev.ps1 game
cargo run -p game --release
```

Requires Vulkan 1.3, a capable GPU driver, and `glslc` for shader builds. Debug initialization also requires the Khronos validation layer.

## Models and scene instances

`AssetManager::load_model` imports **every indexed triangle primitive of the first mesh** from `props/robot.glb`. Each `ModelPart` owns a shared GPU mesh and a material; `Model` groups those parts. Two `ModelInstance` values share one `Arc<Model>` and have independent `Transform` values with position, Euler rotation in radians, and scale.

`Scene::spawn` returns an `EntityId`; `get_mut` permits transform changes, `remove` returns the removed instance, and `instances` supplies drawing iteration. IDs increase without reuse within the scene. There is no scene serialization, hierarchy, general ECS, or editor command integration yet.

On each redraw, the game updates instance transforms, expands every model part into a borrowed `RenderObject`, and calls `VulkanFrame::draw`. Materials retain their own imported color factor and double-sided flag. Shared images reuse GPU texture uploads; factor-only materials use the cached `builtin:white` texture. The fragment shader multiplies sampled color by the material factor.

## Scene modes and packaging

| Mode | Selection | Scene | Asset root |
| --- | --- | --- | --- |
| Development | No readable name file beside executable | Two rotating robot model instances | Compiled manifest directory's `assets` |
| Installed | Readable `project-name.txt` beside executable | Two stationary model instances | `assets` beside executable |

The name file supplies the installed window title. Both modes require `props/robot.glb`. **The compiler still copies only the old `crate.png` and `bricks.jpg` textures**, so fresh installation succeeds but runtime model import fails. For manual inspection, copy the model to `<installed folder>/assets/props/robot.glb` and preserve any external resources needed by other models. See [compiler](../../compiler/README.md).

The admin builder allows GLB/glTF assets and adjusts generated asset lookup. Preserve its generated workspace layout. Import only handles the first mesh: node transforms, additional meshes, animation/skinning, full PBR, and transparent-material behavior are not implemented.

## Renderer lifecycle

`GameApp` owns frame resources, scene, single/double-sided pipeline variants, camera, asset manager, shared material layout, depth, swapchain, device, surface, and instance. Pipeline creation uses renderer-owned layout resources rather than requiring a scene object's material, allowing creation with no draw objects.

Resize or `SwapchainNeedsRebuild` marks presentation dirty; zero-sized windows defer drawing. Rebuild waits for idle and recreates dependent presentation resources. Render/rebuild failures log and exit. Heartbeats occur on redraw roughly every five seconds. `Drop` waits for GPU completion before fields are destroyed; preserve dependency ordering when adding owners. Native empty-scene resize and lifetime behavior still require manual validation.

## Extension and verification

Use `Scene::spawn(ModelInstance::new(...))` for another instance; mutate its transform via its ID. Put reusable GPU geometry/material parts in a `Model`, and preserve device/layout lifetimes while any instance still holds it. See [graphics and assets](../../graphics-and-assets.md) and [renderer](../../crates/renderer/README.md).

From the repository root:

```powershell
cargo check -p game --offline --locked
cargo test -p game --bin game --offline --locked
```

Importer tests cover multiple primitives with distinct materials and unsupported-mode rejection. They do not validate native presentation, scene removal/resize, or complete model formats. See [testing](../../testing.md).
