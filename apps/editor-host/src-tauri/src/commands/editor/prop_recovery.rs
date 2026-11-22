use super::*;
use runtime::linked_prefab::LinkedPrefabRecoveryReview;

#[tauri::command]
pub async fn editor_review_linked_prop_recovery(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	path: String,
	draft_id: String,
	expected_revision: u64,
) -> Result<LinkedPrefabRecoveryReview, String> {
	trusted_window(&window)?;
	workspace(
		state,
		false,
		Some("review_linked_prop_recovery"),
		move |workspace| {
			workspace.review_linked_source_recovery(&root_key, &path, &draft_id, expected_revision)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_recover_linked_prop(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	path: String,
	draft_id: String,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("recover_linked_prop"), move |workspace| {
		workspace.recover_linked_source(&root_key, &path, &draft_id, expected_revision)
	})
	.await
}
