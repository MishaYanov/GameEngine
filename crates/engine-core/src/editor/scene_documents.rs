//! Retained authored documents. Filesystem preparation remains with the host.
mod budget;
use super::*;
#[cfg(test)]
use crate::scene_catalog::MAX_PROJECT_SCENES;
use crate::{command::SceneStateDto, scene_catalog::SceneDescriptor};
pub(super) use budget::DocumentCostCache;
use budget::MAX_RETAINED_DOCUMENT_BYTES;
#[cfg(test)]
use budget::document_cost;

#[derive(Clone)]
pub(super) struct SceneDraft {
	pub data: Arc<SceneData>,
	pub saved: Arc<SceneData>,
	pub saved_path: Option<String>,
	pub selection: Vec<EntityKey>,
}

impl EditorSession {
	pub const MAX_RETAINED_DOCUMENT_BYTES: usize = MAX_RETAINED_DOCUMENT_BYTES;
	pub fn validate_all_documents(&self) -> Result<(), String> {
		for entry in self.project.scenes.iter() {
			let data = self.scene_document(entry.id())?;
			self.project.validate_scene_for(entry.id(), &data)?;
		}
		Ok(())
	}
	pub fn active_scene_id(&self) -> &SceneId {
		&self.active_scene_id
	}

	pub fn scene_document(&self, id: &SceneId) -> Result<Arc<SceneData>, String> {
		self.project.scenes.get(id).map_err(|e| e.to_string())?;
		self.draft_data(id).map(Arc::clone)
	}

	/// Asset replacement must protect references in working, saved and restorable scenes.
	pub fn try_for_each_retained_entity(
		&self,
		mut visit: impl FnMut(&crate::scene_data::EntityData) -> Result<(), String>,
	) -> Result<(), String> {
		let scenes = std::iter::once((&self.data, &self.saved)).chain(
			self.drafts
				.values()
				.map(|draft| (&draft.data, &draft.saved)),
		);
		for (working, saved) in scenes {
			working.entities.iter().try_for_each(&mut visit)?;
			if !Arc::ptr_eq(working, saved) {
				saved.entities.iter().try_for_each(&mut visit)?;
			}
		}
		self.history.retained_entities().try_for_each(visit)
	}

	pub fn scene_states(&self) -> Vec<SceneStateDto> {
		let mut states: Vec<_> = self
			.project
			.scenes
			.iter()
			.map(|entry| {
				let dirty = self.scene_dirty(entry);
				SceneStateDto {
					id: entry.id().clone(),
					asset_path: entry.asset_path().to_owned(),
					loaded: dirty.is_some(),
					dirty: dirty.unwrap_or(false),
				}
			})
			.collect();
		states.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
		states
	}

	pub fn active_scene_dirty(&self) -> bool {
		snapshots_differ(&self.data, &self.saved)
			|| self.saved_path.as_deref() != self.project.scene_path(&self.active_scene_id).ok()
	}

	pub(super) fn scene_dirty(&self, entry: &SceneDescriptor) -> Option<bool> {
		let (data, saved, saved_path) = if entry.id() == &self.active_scene_id {
			(&self.data, &self.saved, self.saved_path.as_deref())
		} else {
			let draft = self.drafts.get(entry.id())?;
			(&draft.data, &draft.saved, draft.saved_path.as_deref())
		};
		Some(snapshots_differ(data, saved) || saved_path != Some(entry.asset_path()))
	}

	fn draft_data(&self, id: &SceneId) -> Result<&Arc<SceneData>, String> {
		if id == &self.active_scene_id {
			Ok(&self.data)
		} else {
			self.drafts
				.get(id)
				.map(|draft| &draft.data)
				.ok_or_else(|| "Scene document is not loaded".into())
		}
	}

	fn active_draft(&self) -> SceneDraft {
		SceneDraft {
			data: Arc::clone(&self.data),
			saved: Arc::clone(&self.saved),
			saved_path: self.saved_path.clone(),
			selection: self.selection.clone(),
		}
	}

	fn draft(&self, id: &SceneId) -> Result<SceneDraft, String> {
		if id == &self.active_scene_id {
			Ok(self.active_draft())
		} else {
			self.drafts
				.get(id)
				.cloned()
				.ok_or_else(|| "Scene document is not loaded".into())
		}
	}

	/// Used only while constructing/replacing a candidate workspace.
	pub fn load_saved_scenes(
		&mut self,
		documents: Vec<(SceneId, SceneData)>,
	) -> Result<(), String> {
		let mut staged = self.drafts.clone();
		let mut unique = HashSet::new();
		for (id, mut data) in documents {
			if !unique.insert(id.clone()) || id == self.active_scene_id || staged.contains_key(&id)
			{
				return Err("A loaded scene ID must be distinct and inactive".into());
			}
			data.refresh_game_camera_projection()?;
			self.project.validate_scene_for(&id, &data)?;
			let data = Arc::new(data);
			staged.insert(
				id.clone(),
				SceneDraft {
					saved: Arc::clone(&data),
					data,
					saved_path: Some(self.project.scene_path(&id)?.to_owned()),
					selection: Vec::new(),
				},
			);
		}
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&staged,
			None,
			&mut self.document_costs,
		)?;
		self.drafts = staged;
		Ok(())
	}

	pub fn mark_scene_saved_by_id(&mut self, id: &SceneId) -> Result<(), String> {
		self.require_no_transaction()?;
		self.ensure_revision_capacity()?;
		if id == &self.active_scene_id {
			return self.mark_scene_saved();
		}
		let path = self.project.scene_path(id)?.to_owned();
		let draft = self
			.drafts
			.get_mut(id)
			.ok_or("Scene document is not loaded")?;
		if snapshots_differ(&draft.data, &draft.saved) || draft.saved_path.as_deref() != Some(&path)
		{
			draft.saved = Arc::clone(&draft.data);
			draft.saved_path = Some(path);
			self.bump();
		}
		Ok(())
	}

	fn activate(&mut self, id: SceneId, draft: SceneDraft) {
		if id != self.active_scene_id {
			let outgoing = self.active_draft();
			self.drafts.insert(self.active_scene_id.clone(), outgoing);
			self.drafts.remove(&id);
			self.active_scene_id = id;
		}
		self.data = draft.data;
		self.saved = draft.saved;
		self.saved_path = draft.saved_path;
		self.selection = draft.selection;
		self.scene_revision += 1;
	}

	pub(super) fn switch_scene(&mut self, id: SceneId) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		self.project.scenes.get(&id).map_err(|e| e.to_string())?;
		if id == self.active_scene_id {
			return Ok(());
		}
		let draft = self.draft(&id)?;
		self.project.validate_scene_for(&id, &draft.data)?;
		validate_selection(&draft.data, &draft.selection)?;
		self.check_document_budget(None)?;
		self.activate(id, draft);
		self.bump();
		engine_logger::info!(target: "engine_core::editor", "Authored scene switched: revision={}", self.revision);
		Ok(())
	}

	pub(super) fn add_scene(
		&mut self,
		id: SceneId,
		path: String,
		data: SceneData,
	) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		self.prune_retired_documents();
		if self.drafts.contains_key(&id) || id == self.active_scene_id {
			return Err("Scene ID is retained by document history; generate a new ID".into());
		}
		let mut project = (*self.project).clone();
		project
			.scenes
			.insert(SceneDescriptor::new(id.clone(), path)?)
			.map_err(|e| e.to_string())?;
		project.validate_scene_for(&id, &data)?;
		let data = Arc::new(data);
		let draft = SceneDraft {
			data: Arc::clone(&data),
			saved: data,
			saved_path: None,
			selection: Vec::new(),
		};
		let mut staged = self.drafts.clone();
		staged.insert(id.clone(), draft.clone());
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&staged,
			None,
			&mut self.document_costs,
		)?;

		self.commit_project(
			(*self.data).clone(),
			project,
			self.selection.clone(),
			"Add scene",
		)?;
		self.drafts.insert(id, draft);
		Ok(())
	}

	pub(super) fn remove_scene(&mut self, id: SceneId) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		if id == self.active_scene_id || id == self.project.startup_scene_id {
			return Err(
				"Switch away and choose another startup scene before removing this scene".into(),
			);
		}
		self.project.scenes.get(&id).map_err(|e| e.to_string())?;
		if self.scene_states().iter().any(|s| s.id == id && s.dirty) {
			return Err("Save the scene before removing it from the catalog".into());
		}
		let mut project = (*self.project).clone();
		project.scenes.remove(&id).map_err(|e| e.to_string())?;
		project.behaviors.retain(|b| b.scene_id != id);
		self.commit_project(
			(*self.data).clone(),
			project,
			self.selection.clone(),
			"Remove scene",
		)
	}

	pub(super) fn set_startup_scene(&mut self, id: SceneId) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		self.project.scenes.get(&id).map_err(|e| e.to_string())?;
		let mut project = (*self.project).clone();
		project.startup_scene_id = id;
		self.commit_project(
			(*self.data).clone(),
			project,
			self.selection.clone(),
			"Set startup scene",
		)
	}

	pub(super) fn change_scene_path(&mut self, id: SceneId, path: String) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let mut project = (*self.project).clone();
		project
			.scenes
			.change_path(&id, path)
			.map_err(|e| e.to_string())?;
		self.commit_project(
			(*self.data).clone(),
			project,
			self.selection.clone(),
			"Change scene path",
		)
	}

	pub(super) fn validate_project_documents(
		&self,
		project: &ProjectData,
		id: &SceneId,
		data: &SceneData,
	) -> Result<(), String> {
		project.validate_scene_for(id, data)?;
		for entry in project.scenes.iter() {
			if entry.id() != id
				&& let Ok(data) = self.draft_data(entry.id())
			{
				project.validate_scene_for(entry.id(), data)?;
			}
		}
		Ok(())
	}

	pub(super) fn restore_history(&mut self, redo: bool) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let Some(id) = self.history.target_scene(redo).cloned() else {
			return Ok(());
		};
		let mut draft = self.draft(&id)?;
		let Some((data, project, selection)) =
			self.history
				.preview_restore(&draft.data, &self.project, redo)?
		else {
			return Ok(());
		};
		self.validate_project_documents(&project, &id, &data)?;
		validate_selection(&data, &selection)?;
		self.check_document_budget(Some((&id, &data)))?;
		self.history.commit_restore(redo);
		draft.data = Arc::new(data);
		draft.selection = selection;
		self.activate(id, draft);
		self.project = Arc::new(project);
		self.bump();
		engine_logger::debug!(target: "engine_core::editor", "History restored: redo={redo}, revision={}", self.revision);
		Ok(())
	}

	fn prune_retired_documents(&mut self) {
		let retained = self.history.retained_scene_ids();
		self.drafts
			.retain(|id, _| self.project.scenes.get(id).is_ok() || retained.contains(id));
	}
}

#[cfg(test)]
#[path = "../../tests/unit/editor/scene_documents.rs"]
mod tests;
