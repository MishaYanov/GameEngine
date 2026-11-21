//! Bounded read-only animation review. Native source identity is returned with the
//! reviewed bytes; authoring changes continue through revision-checked components.
use super::*;
use runtime::asset_catalog::AssetId;

#[derive(Serialize)]
pub struct AnimationReviewDto {
	pub workspace_generation: u64,
	pub revision: u64,
	pub metadata: bridge::animation_asset::AnimationMetadata,
}

fn review_context(workspace: &EditorWorkspace, expected_revision: u64) -> Result<(), String> {
	let session = workspace.backend.session();
	if session.revision() != expected_revision {
		return Err("Revision conflict: review the animation again for the current scene".into());
	}
	if session.is_playing() || session.has_transaction() {
		return Err("Stop Play and finish the current edit before reviewing animation".into());
	}
	Ok(())
}

#[tauri::command]
pub async fn editor_animation_metadata(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: String,
	expected_revision: u64,
) -> Result<AnimationReviewDto, String> {
	trusted_window(&window)?;
	let asset_id = AssetId::new(asset_id)?;
	workspace(state, false, Some("animation_metadata"), move |workspace| {
		review_context(workspace, expected_revision)?;
		let metadata = bridge::animation_asset::inspect(&workspace.asset_root, &asset_id)?;
		Ok(AnimationReviewDto {
			workspace_generation: workspace.generation,
			revision: workspace.backend.session().revision(),
			metadata,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_animation_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	entity_key: String,
	clip_index: u32,
	expected_revision: u64,
) -> Result<crate::animation_preview::AnimationPreviewDto, String> {
	trusted_window(&window)?;
	let key = runtime::EntityKey::new(entity_key)?;
	workspace(
		state,
		true,
		Some("animation_preview_open"),
		move |workspace| workspace.open_animation_preview(key, clip_index, expected_revision),
	)
	.await
}
#[tauri::command]
pub async fn editor_animation_preview_sample(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	time_seconds: f64,
) -> Result<crate::animation_preview::AnimationPreviewDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("animation_preview_sample"),
		move |workspace| workspace.sample_animation_preview(&token, time_seconds),
	)
	.await
}
#[tauri::command]
pub async fn editor_animation_preview_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, true, None, move |workspace| {
		workspace.close_animation_preview(&token);
		Ok(())
	})
	.await
}
