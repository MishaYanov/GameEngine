use super::*;
use crate::editor::prefab_sources::nested::{NestedPropPlacementDto, NestedPropSourceDto};
use runtime::asset_catalog::AssetId;

#[tauri::command]
pub async fn editor_prop_nested_source(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	fingerprint: String,
	expected_generation: u64,
) -> Result<NestedPropSourceDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("prop_nested_source"), move |workspace| {
		if workspace.generation != expected_generation {
			return Err("The workspace changed. Reopen the nested Prop picker.".into());
		}
		workspace.nested_prop_source(&asset_id, &fingerprint)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_draft_add_nested(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	placement: NestedPropPlacementDto,
) -> Result<crate::editor::prefab_sources::PrefabSourceDraftDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("prop_draft_add_nested"),
		move |workspace| {
			workspace.add_prop_draft_placement(&draft_id, expected_draft_revision, placement)
		},
	)
	.await
}
