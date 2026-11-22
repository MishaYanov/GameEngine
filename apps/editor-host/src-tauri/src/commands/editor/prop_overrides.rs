use super::*;
use crate::editor::prefab_sources::PrefabSourceSaveDto;
use runtime::{
	linked_prefab::{
		LinkedPrefabOverrideResolution, LinkedPrefabOverrideReviewDto, LinkedPrefabProperty,
		PropAddress,
	},
	EntityKey,
};
use std::collections::BTreeMap;

#[tauri::command]
pub async fn editor_review_linked_prop_overrides(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	draft_id: String,
	bindings: Option<BTreeMap<String, EntityKey>>,
	expected_revision: u64,
) -> Result<Vec<LinkedPrefabOverrideReviewDto>, String> {
	trusted_window(&window)?;
	workspace(
		state,
		false,
		Some("review_linked_prop_overrides"),
		move |workspace| {
			workspace.review_linked_source_overrides(
				&root_key,
				&draft_id,
				bindings,
				expected_revision,
			)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_update_linked_prop_reviewed(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	draft_id: String,
	bindings: Option<BTreeMap<String, EntityKey>>,
	resolutions: Vec<LinkedPrefabOverrideResolution>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("update_linked_prop_reviewed"),
		move |workspace| {
			workspace.update_linked_source_reviewed(
				&root_key,
				&draft_id,
				bindings,
				resolutions,
				expected_revision,
			)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_apply_linked_prop_override(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	draft_id: String,
	address: PropAddress,
	property: LinkedPrefabProperty,
	expected_revision: u64,
) -> Result<PrefabSourceSaveDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("apply_linked_prop_override"),
		move |workspace| {
			workspace.apply_linked_override_to_source(
				&root_key,
				&draft_id,
				&address,
				&property,
				expected_revision,
			)
		},
	)
	.await
}
