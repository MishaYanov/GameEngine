//! The trusted native window owns consent and revocation for the external MCP connection.

use super::*;
use crate::{mcp_access::Connection, release_resources::AgentResources};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[tauri::command]
pub async fn editor_mcp_grant(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
) -> Result<Option<Connection>, String> {
	trusted_window(&window)?;
	let page = state.mcp_access.page()?;
	let access = Arc::clone(&state.mcp_access);
	let shared = Arc::clone(&state.editor);
	let exit = Arc::clone(&state.exit);
	tauri::async_runtime::spawn_blocking(move || {
		{
			let slot = shared.lock().map_err(|_| "The workspace is unavailable")?;
			if !slot.as_ref().is_some_and(|workspace| workspace.generation == workspace_generation) {
				return Err("The project changed. Request access again.".into());
			}
		}
		let accepted = window.dialog()
			.message("A local MCP client with the connection token can read the current scene selection and project asset names and paths. Access expires in 10 minutes and ends when you revoke it or change projects.")
			.title("Allow external Agent access?")
			.parent(&window)
			.buttons(MessageDialogButtons::OkCancelCustom("Allow read access".into(), "Cancel".into()))
			.blocking_show();
		if !accepted {
			return Ok(None);
		}
		trusted_window(&window)?;
		let worker_path = AgentResources::for_window(&window).ok()
			.map(|resources| resources.executable.to_string_lossy().into_owned());
		let slot = shared.lock().map_err(|_| "The workspace is unavailable")?;
		let workspace = slot.as_ref().filter(|workspace| workspace.generation == workspace_generation)
			.ok_or("The project changed. Request access again.")?;
		access.issue(page, workspace, Arc::clone(&shared), exit, worker_path).map(Some)
	})
	.await
	.map_err(|_| "The external Agent access request stopped".to_owned())?
}

#[tauri::command]
pub fn editor_mcp_revoke(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
	token: String,
) -> Result<bool, String> {
	trusted_window(&window)?;
	Ok(state.mcp_access.revoke(workspace_generation, &token))
}
