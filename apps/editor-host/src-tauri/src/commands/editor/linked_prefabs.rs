//! Typed source drafts and explicit link boundaries. No source file watcher
//! mutates an authored instance; updates remain undoable commands.
use super::*;
use crate::editor::prefab_sources::{
	LinkedPrefabStatusDto, LinkedPropAddressDto, PrefabSourceDraftDto, PrefabSourceSaveDto,
	PrefabSourceValidationDto, PropDraftEdit,
};
use runtime::{linked_prefab::PrefabConflictResolution, EntityKey};
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(untagged)]
pub enum LinkedPrefabInspectionDto {
	Status(LinkedPrefabStatusDto),
	Address(LinkedPropAddressDto),
}

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
pub async fn editor_prop_draft_edit(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
	edit: PropDraftEdit,
) -> Result<PrefabSourceDraftDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("prop_draft_edit"), move |w| {
		w.edit_prop_draft(&draft_id, expected_draft_revision, edit)
	})
	.await
}

#[tauri::command]
pub async fn editor_prop_draft_save(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	draft_id: String,
	expected_draft_revision: u64,
) -> Result<PrefabSourceSaveDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("prop_draft_save"), move |w| {
		w.save_prop_draft(&draft_id, expected_draft_revision)
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
	address_only: Option<bool>,
	expected_generation: Option<u64>,
) -> Result<Option<LinkedPrefabInspectionDto>, String> {
	trusted_window(&window)?;
	if address_only == Some(true) {
		let generation =
			expected_generation.ok_or("The Inspector subject needs a workspace generation")?;
		let shared = Arc::clone(&state.editor);
		return tauri::async_runtime::spawn_blocking(move || {
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			let workspace = slot
				.as_ref()
				.ok_or("Open a scene before using the editor")?;
			workspace
				.linked_prop_address(&root_key, generation, expected_revision)
				.map(|address| address.map(LinkedPrefabInspectionDto::Address))
		})
		.await
		.map_err(|error| format!("Editor worker failed: {error}"))?;
	}
	workspace(state, false, None, move |w| {
		w.linked_prefab_status(&root_key, expected_revision)
			.map(|status| status.map(LinkedPrefabInspectionDto::Status))
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
