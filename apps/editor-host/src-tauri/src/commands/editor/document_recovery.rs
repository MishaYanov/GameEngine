use super::*;
use crate::editor::recovery::RecoveryOffer;

#[derive(Serialize)]
pub struct RecoveryReview {
	pub workspace_generation: u64,
	pub revision: u64,
	pub offer: Option<RecoveryOffer>,
}

#[tauri::command]
pub async fn editor_review_document_recovery(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<RecoveryReview, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |workspace| {
		workspace.check_revision(expected_revision)?;
		let offer = workspace.review_recovery()?;
		Ok(RecoveryReview {
			workspace_generation: workspace.generation,
			revision: expected_revision,
			offer,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_apply_document_recovery(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	expected_revision: u64,
	token: [u8; 32],
	restore: bool,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("apply_document_recovery"),
		move |workspace| {
			if workspace.generation != expected_generation {
				return Err("The open project changed. Review recovery again.".into());
			}
			workspace.apply_recovery(expected_revision, token, restore)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_checkpoint_documents(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
) -> Result<bool, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let capture = {
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			let Some(workspace) = slot
				.as_ref()
				.filter(|w| w.generation == expected_generation)
			else {
				return Ok(false);
			};
			workspace.capture_recovery()?
		};
		let Some(capture) = capture else {
			return Ok(false);
		};
		let staged = capture.stage()?;
		let mut slot = shared.lock().map_err(|_| "Editor state lock failed")?;
		match slot.as_mut() {
			Some(workspace) => workspace.publish_recovery(staged),
			None => Ok(false),
		}
	})
	.await
	.map_err(|error| error.to_string())?
}
