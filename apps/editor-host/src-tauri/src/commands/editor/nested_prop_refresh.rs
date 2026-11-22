use super::*;
use crate::editor::prefab_sources::PrefabSourceDraftDto;
use runtime::linked_prefab::PlacementId;

#[tauri::command]
pub async fn editor_prop_draft_refresh_nested(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	placement_id: PlacementId,
	fingerprint: String,
) -> Result<PrefabSourceDraftDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		true,
		Some("prop_draft_refresh_nested"),
		move |workspace| {
			workspace.refresh_prop_draft_placement(
				&draft_id,
				expected_draft_revision,
				&placement_id,
				&fingerprint,
			)
		},
	)
	.await
}
