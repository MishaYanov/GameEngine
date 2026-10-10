use bridge::EngineStatusDto;
use engine_logger::ResultLogExt;
use tauri::State;

use crate::state::EditorState;

#[tauri::command]
pub fn engine_status(state: State<'_, EditorState>) -> Result<EngineStatusDto, String> {
    let engine = state
        .engine
        .lock()
        .log_error_for("editor-host::commands", "acquire engine state")
        .map_err(|_| "failed to acquire engine state".to_string())?;

    Ok(engine.status())
}
