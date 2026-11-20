use crate::state::EditorState;
use tauri::{State, WebviewWindow};

#[tauri::command]
pub fn editor_runtime_inspect(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	offset: u32,
	limit: u32,
) -> Result<runtime::runtime_lifecycle::RuntimeInspectionDto, String> {
	super::editor::trusted_window(&window)?;
	let guard = state
		.editor
		.lock()
		.map_err(|_| "Editor state lock failed")?;
	let workspace = guard
		.as_ref()
		.ok_or("Open a project before inspecting runtime entities")?;
	workspace
		.backend
		.runtime_inspection(offset as usize, limit as usize)
}
