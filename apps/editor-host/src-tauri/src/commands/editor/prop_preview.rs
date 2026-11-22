use super::*;
use bridge::viewport::{PropPreviewStateDto, PropPreviewTargetDto};
use runtime::EntityKey;

#[tauri::command]
pub async fn editor_prop_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	target: PropPreviewTargetDto,
	expected_revision: u64,
	expected_generation: u64,
	request_id: u64,
) -> Result<PropPreviewStateDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("prop_preview_open"), move |workspace| {
		workspace.open_prop_preview(target, expected_revision, expected_generation, request_id)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.close_prop_preview(&token);
		Ok(())
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_frame(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.frame_prop_preview(&token)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_preview_select(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	key: Option<EntityKey>,
) -> Result<PropPreviewStateDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.select_prop_preview(&token, key)
	})
	.await
}
