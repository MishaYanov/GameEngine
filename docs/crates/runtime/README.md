# Engine runtime

Source: `crates/runtime`. Cargo package: `runtime`. The runtime wraps renderer initialization and exposes engine status to the editor host.

## API and ownership

`Engine::new() -> Result<Engine, EngineError>` constructs a `renderer::Renderer` and emits lifecycle diagnostics. `Engine::status() -> bridge::EngineStatusDto` reports initialization, renderer name, GPU name, and Vulkan API. `EngineError::Renderer` preserves the cause through `Error::source`.

Inside a fallible application function:

```rust
let engine = runtime::Engine::new()?;
let status = engine.status();
println!("{}: {} ({})", status.renderer, status.gpu_name, status.vulkan_api);
```

The executable initializes the shared logger before constructing the engine and retains its guard. Initialization requires [Vulkan prerequisites](../renderer/README.md) even though this wrapper opens no graphics window.

`Engine` owns its renderer. The host wraps it in a mutex; the game owns low-level renderer resources directly. There is currently no tick/update API, entity storage, physics, or scene-loading contract.

## Verification

From the repository root:

```powershell
cargo check -p runtime --offline --locked
cargo test -p runtime --offline --locked
```

The crate has no behavioral Rust tests. Compilation does not verify GPU initialization; use the [renderer probe](../renderer/README.md) and desktop checks. See [architecture](../../architecture.md) and [testing](../../testing.md).
