use bridge::EngineStatusDto;
use tauri::State;

use crate::state::EditorState;

#[tauri::command]
pub fn engine_status(
	state: State<'_, EditorState>,
) -> Result<EngineStatusDto, String> {
	let engine = state
		.engine
		.lock()
		.map_err(|_| {
			"failed to acquire engine state".to_string()
		})?;

	Ok(engine.status())
}