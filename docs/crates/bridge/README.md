# Shared engine contracts

Source: `crates/bridge`. Cargo package: `bridge`. This crate provides Rust DTOs returned to the frontend. It currently contains `EngineStatusDto` and implements no IPC transport or entity commands.

## Engine status contract

`EngineStatusDto` derives `Debug`, `Clone`, and `Serialize`. Serde uses camelCase JSON names:

| Rust field | JSON field | Type |
| --- | --- | --- |
| `initialized` | `initialized` | Boolean |
| `renderer` | `renderer` | String |
| `gpu_name` | `gpuName` | String |
| `vulkan_api` | `vulkanApi` | String |

Runtime constructs the value, the host returns it from `engine_status`, and the editor models it in `apps/editor/src/app/core/engine/engine.models.ts`. Coordinate changes across these consumers. Values describe the initialized renderer rather than an embedded editor viewport.

## Extension and verification

Keep shared serializable engine data here and platform/UI behavior in applications. Preserve JSON names or update frontend types/callers together. The status type currently defines no version migration or deserialization contract.

From the repository root:

```powershell
cargo check -p bridge --offline --locked
cargo test -p bridge --offline --locked
```

A serialization test verifies the camelCase engine-status JSON contract. See [host](../../apps/editor-host/README.md), [runtime](../runtime/README.md), and [architecture](../../architecture.md).
