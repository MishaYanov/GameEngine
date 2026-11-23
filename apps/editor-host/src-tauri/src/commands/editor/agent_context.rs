//! Trusted local IPC adapter for the read-only authored Agent snapshot.
use super::*;
use crate::editor::agent_context::{
	AgentContextError, AgentContextErrorCode, AgentContextRequest, AgentObjectSummary,
};

#[tauri::command]
pub async fn editor_agent_context(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: AgentContextRequest,
) -> Result<AgentObjectSummary, AgentContextError> {
	use AgentContextErrorCode::{Forbidden, Missing, Unavailable};

	trusted_window(&window).map_err(|_| {
		AgentContextError::new(
			Forbidden,
			"Agent context requires the trusted Editor window",
		)
	})?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let capture = {
			let slot = shared.lock().map_err(|_| {
				AgentContextError::new(Unavailable, "The authored workspace is unavailable")
			})?;
			let workspace = slot.as_ref().ok_or_else(|| {
				AgentContextError::new(Missing, "Open a project before inspecting an object")
			})?;
			workspace.capture_agent_context(request)?
		};
		capture.resolve()
	})
	.await
	.map_err(|_| AgentContextError::new(Unavailable, "The Agent context worker stopped"))?
}
