use super::*;
use crate::animation_preview::{
	authored::AuthoredClipPreviewDto,
	timeline::{ClipDraftDto, ClipDraftOperation, ClipDraftSaved, PreviewKeyMove},
	AnimationPreviewDocument,
};
use runtime::asset_catalog::AssetId;

#[tauri::command]
pub async fn editor_clip_timeline_draft_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	expected_revision: u64,
) -> Result<ClipDraftDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |workspace| {
		workspace.open_clip_timeline_draft(&asset_id, expected_revision)
	})
	.await
}

#[tauri::command]
pub async fn editor_clip_timeline_draft_edit(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	operation: ClipDraftOperation,
) -> Result<ClipDraftDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.edit_clip_timeline_draft(&draft_id, expected_draft_revision, operation)
	})
	.await
}

#[tauri::command]
pub async fn editor_clip_timeline_draft_save(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	expected_revision: u64,
) -> Result<ClipDraftSaved, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("save_clip_timeline"), move |workspace| {
		workspace.save_clip_timeline_draft(&draft_id, expected_draft_revision, expected_revision)
	})
	.await
}

#[tauri::command]
pub async fn editor_clip_timeline_draft_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	discard: bool,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.close_clip_timeline_draft(&draft_id, discard)
	})
	.await
}

#[tauri::command]
#[expect(
	clippy::too_many_arguments,
	reason = "Keep the existing Tauri IPC fields at the top level of the request"
)]
pub async fn editor_clip_timeline_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	scope: Option<AnimationPreviewDocument>,
	expected_revision: u64,
	expected_generation: u64,
	request_id: u64,
) -> Result<AuthoredClipPreviewDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.open_clip_timeline_preview(
			&draft_id,
			expected_draft_revision,
			scope,
			expected_revision,
			expected_generation,
			request_id,
		)
	})
	.await
}

#[tauri::command]
pub async fn editor_clip_timeline_preview_sample(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	tick: u32,
	drag: Option<PreviewKeyMove>,
) -> Result<AuthoredClipPreviewDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.sample_clip_timeline_preview(&token, tick, drag)
	})
	.await
}
