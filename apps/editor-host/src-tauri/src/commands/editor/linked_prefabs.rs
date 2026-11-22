//! Typed source drafts and explicit link boundaries. No source file watcher
//! mutates an authored instance; updates remain undoable commands.
use super::*;
use crate::editor::prefab_sources::{
	LinkedPrefabStatusDto, PrefabSourceDraftDto, PrefabSourceSaveDto, PrefabSourceValidationDto,
};
use runtime::{EntityKey, linked_prefab::PrefabConflictResolution};
use std::collections::BTreeMap;

#[tauri::command]
pub async fn editor_prefab_source_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<PrefabSourceDraftDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("prefab_source_open"), move |w| {
		w.open_prefab_source(&path, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_prefab_source_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |w| {
		w.close_prefab_source(&draft_id)
	})
	.await
}
#[tauri::command]
pub async fn editor_prefab_source_validate(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	prefab: serde_json::Value,
	expected_revision: u64,
) -> Result<PrefabSourceValidationDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("prefab_source_validate"), move |w| {
		w.validate_prefab_source(&draft_id, prefab, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_prefab_source_save(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	prefab: serde_json::Value,
	expected_revision: u64,
) -> Result<PrefabSourceSaveDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("prefab_source_save"), move |w| {
		w.save_prefab_source(&draft_id, prefab, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_create_linked_prefab_source(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	reference_slots: Option<BTreeMap<String, EntityKey>>,
	pivot: Option<EntityKey>,
	expected_revision: u64,
) -> Result<PrefabSourceDraftDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		false,
		Some("create_linked_prefab_source"),
		move |w| {
			let slots = reference_slots.unwrap_or_default();
			match pivot.as_ref() {
				Some(pivot) => {
					w.create_linked_prefab_source_at(&path, slots, Some(pivot), expected_revision)
				}
				None => w.create_linked_prefab_source(&path, slots, expected_revision),
			}
		},
	)
	.await
}
#[tauri::command]
pub async fn editor_instantiate_linked_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	draft_id: String,
	position: [f32; 3],
	bindings: Option<BTreeMap<String, EntityKey>>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("instantiate_linked_prefab"), move |w| {
		w.instantiate_linked_source(
			&path,
			&draft_id,
			position,
			bindings.unwrap_or_default(),
			expected_revision,
		)
	})
	.await
}
#[tauri::command]
pub async fn editor_linked_prefab_status(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	expected_revision: u64,
) -> Result<Option<LinkedPrefabStatusDto>, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |w| {
		w.linked_prefab_status(&root_key, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_update_linked_prefab(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	root_key: String,
	draft_id: String,
	resolution: PrefabConflictResolution,
	bindings: Option<BTreeMap<String, EntityKey>>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("update_linked_prefab"), move |w| {
		w.update_linked_source(
			&root_key,
			&draft_id,
			resolution,
			bindings,
			expected_revision,
		)
	})
	.await
}
