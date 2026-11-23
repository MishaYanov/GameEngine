use super::*;
use crate::animation_preview::{
	authored::{
		AuthoredClipPreviewDto, ClipAssetDto, ClipAssetIndex, ClipAssetSave, ClipReplacement,
	},
	AnimationPreviewDocument,
};
use runtime::{animation::AuthoredClip, asset_catalog::AssetId};

#[tauri::command]
pub async fn editor_animation_clips(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<Vec<ClipAssetIndex>, String> {
	trusted_window(&window)?;
	workspace(state, false, None, |workspace| {
		workspace.authored_clip_assets()
	})
	.await
}

#[tauri::command]
pub async fn editor_open_authored_clip(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
) -> Result<ClipAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |workspace| {
		workspace.open_authored_clip_asset(&asset_id)
	})
	.await
}

#[tauri::command]
pub async fn editor_save_authored_clip(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	clip: AuthoredClip,
	replacement: Option<ClipReplacement>,
	expected_revision: u64,
	expected_generation: u64,
) -> Result<ClipAssetSave, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("save_authored_clip"), move |workspace| {
		if workspace.generation != expected_generation {
			return Err("The authored clip belongs to another workspace".into());
		}
		workspace.save_authored_clip_asset(&path, clip, replacement, expected_revision)
	})
	.await
}

#[tauri::command]
#[expect(
	clippy::too_many_arguments,
	reason = "Keep the existing Tauri IPC fields at the top level of the request"
)]
pub async fn editor_authored_clip_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	fingerprint: String,
	scope: Option<AnimationPreviewDocument>,
	expected_revision: u64,
	expected_generation: u64,
	request_id: u64,
) -> Result<AuthoredClipPreviewDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("authored_clip_preview_open"),
		move |workspace| {
			workspace.open_authored_clip_preview(
				&asset_id,
				&fingerprint,
				scope,
				expected_revision,
				expected_generation,
				request_id,
			)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_authored_clip_preview_sample(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	tick: u32,
) -> Result<AuthoredClipPreviewDto, String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.sample_authored_clip_preview(&token, tick)
	})
	.await
}

#[tauri::command]
pub async fn editor_authored_clip_preview_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.close_authored_clip_preview(&token);
		Ok(())
	})
	.await
}
