use super::{trusted_window, Arc, EditorState, State, WebviewWindow};
use crate::editor::scene_verification::SceneVerificationReport;
use runtime::scene_catalog::SceneId;

#[tauri::command]
pub async fn editor_check_scenes(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: Option<u64>,
	expected_revision: u64,
	scene_id: SceneId,
) -> Result<SceneVerificationReport, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let input = {
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			let workspace = slot.as_ref().ok_or("Open a scene before checking scenes")?;
			workspace.capture_scene_verification(
				expected_generation,
				expected_revision,
				&scene_id,
			)?
		};
		let report = input.verify()?;
		let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
		let workspace = slot.as_ref().ok_or("Open a scene before checking scenes")?;
		workspace.check_scene_verification_current(
			Some(report.generation),
			report.revision,
			&scene_id,
		)?;
		Ok(report)
	})
	.await
	.map_err(|error| format!("Scene check worker failed: {error}"))?
}
