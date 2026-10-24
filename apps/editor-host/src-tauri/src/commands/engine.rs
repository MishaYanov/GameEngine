use bridge::EngineStatusDto;
use std::sync::Arc;
use tauri::State;

use crate::state::EditorState;

#[tauri::command]
pub async fn engine_status(
	window: tauri::WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<EngineStatusDto, String> {
	super::editor::trusted_window(&window)?;
	let shared = Arc::clone(&state.engine_status);
	tauri::async_runtime::spawn_blocking(move || {
		let mut cached = shared
			.lock()
			.map_err(|_| "Failed to acquire engine status state")?;
		if let Some(status) = cached.as_ref() {
			return Ok(status.clone());
		}
		// A status query needs device information, not a second permanently
		// allocated renderer beside the actual embedded viewport.
		let status = runtime::GraphicsProbe::probe_status().map_err(|error| {
			engine_logger::error!("Engine status probe failed: {error}");
			error.to_string()
		})?;
		*cached = Some(status.clone());
		Ok(status)
	})
	.await
	.map_err(|error| format!("Engine status worker failed: {error}"))?
}
