//! Retained authored documents. Filesystem preparation remains with the host.
use super::*;
use crate::{
	command::SceneStateDto,
	scene_catalog::{MAX_PROJECT_SCENES, SceneDescriptor},
};

pub const MAX_RETAINED_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct SceneDraft {
	pub data: Arc<SceneData>,
	pub saved: Arc<SceneData>,
	pub saved_path: Option<String>,
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
		Ok(self.draft(id)?.data)
	}

	pub fn scene_states(&self) -> Vec<SceneStateDto> {
		let mut states: Vec<_> = self
			.project
			.scenes
			.iter()
			.map(|entry| {
				let draft = self.draft(entry.id()).ok();
				SceneStateDto {
					id: entry.id().clone(),
					asset_path: entry.asset_path().to_owned(),
					loaded: draft.is_some(),
					dirty: draft.is_some_and(|d| {
						d.data != d.saved || d.saved_path.as_deref() != Some(entry.asset_path())
					}),
				}
			})
			.collect();
		states.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
		states
	}

	pub fn active_scene_dirty(&self) -> bool {
		self.data != self.saved
			|| self.saved_path.as_deref() != self.project.scene_path(&self.active_scene_id).ok()
	}

	fn draft(&self, id: &SceneId) -> Result<SceneDraft, String> {
		if id == &self.active_scene_id {
			Ok(SceneDraft {
				data: Arc::clone(&self.data),
				saved: Arc::clone(&self.saved),
				saved_path: self.saved_path.clone(),
			})
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
		for (id, data) in documents {
			if !unique.insert(id.clone()) || id == self.active_scene_id || staged.contains_key(&id)
			{
				return Err("A loaded scene ID must be distinct and inactive".into());
			}
			self.project.validate_scene_for(&id, &data)?;
			let data = Arc::new(data);
			staged.insert(
				id.clone(),
				SceneDraft {
					saved: Arc::clone(&data),
					data,
					saved_path: Some(self.project.scene_path(&id)?.to_owned()),
				},
			);
		}
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&staged,
			None,
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
		if draft.data != draft.saved || draft.saved_path.as_deref() != Some(&path) {
			draft.saved = Arc::clone(&draft.data);
			draft.saved_path = Some(path);
			self.bump();
		}
		Ok(())
	}

	fn activate(&mut self, id: SceneId, draft: SceneDraft) {
		if id != self.active_scene_id {
			self.drafts.insert(
				self.active_scene_id.clone(),
				SceneDraft {
					data: Arc::clone(&self.data),
					saved: Arc::clone(&self.saved),
					saved_path: self.saved_path.clone(),
				},
			);
			self.drafts.remove(&id);
			self.active_scene_id = id;
		}
		self.data = draft.data;
		self.saved = draft.saved;
		self.saved_path = draft.saved_path;
		self.selection.clear();
		// Bounds describe project-relative assets, so they remain valid across
		// scene navigation and same-scene history restoration.
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
		self.check_document_budget(None)?;
		self.activate(id, draft);
		self.bump();
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
		};
		let mut staged = self.drafts.clone();
		staged.insert(id.clone(), draft.clone());
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&staged,
			None,
		)?;
		// History is accepted before either catalog or document membership commits.
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
				&& let Ok(draft) = self.draft(entry.id())
			{
				project.validate_scene_for(entry.id(), &draft.data)?;
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
		let Some((data, project, _)) =
			self.history
				.preview_restore(&draft.data, &self.project, redo)?
		else {
			return Ok(());
		};
		self.validate_project_documents(&project, &id, &data)?;
		self.check_document_budget(Some((&id, &data)))?;
		let Some((data, project, selection)) =
			self.history.restore(&draft.data, &self.project, redo)?
		else {
			return Ok(());
		};
		draft.data = Arc::new(data);
		self.activate(id, draft);
		self.project = Arc::new(project);
		self.selection = selection;
		self.bump();
		Ok(())
	}

	fn prune_retired_documents(&mut self) {
		let retained = self.history.retained_scene_ids();
		self.drafts
			.retain(|id, _| self.project.scenes.get(id).is_ok() || retained.contains(id));
	}

	pub(super) fn check_document_budget(
		&self,
		replacement: Option<(&SceneId, &SceneData)>,
	) -> Result<(), String> {
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&self.drafts,
			replacement,
		)
	}

	fn document_budget(
		active: &SceneId,
		data: &Arc<SceneData>,
		saved: &Arc<SceneData>,
		drafts: &HashMap<SceneId, SceneDraft>,
		replacement: Option<(&SceneId, &SceneData)>,
	) -> Result<(), String> {
		if drafts.len() + 1 > MAX_PROJECT_SCENES {
			return Err("Retained scene count limit exceeded".into());
		}
		// Conservative logical document accounting, not process/driver memory.
		fn cost(data: &SceneData) -> Result<usize, String> {
			Ok(data
				.to_json()?
				.len()
				.saturating_mul(4)
				.saturating_add(std::mem::size_of::<SceneData>()))
		}
		let mut bytes = cost(
			if let Some((id, next)) = replacement.filter(|(id, _)| *id == active) {
				let _ = id;
				next
			} else {
				data
			},
		)?;
		if !Arc::ptr_eq(data, saved) || replacement.is_some_and(|(id, _)| id == active) {
			bytes = bytes.saturating_add(cost(saved)?);
		}
		for (id, draft) in drafts {
			let next = replacement
				.filter(|(key, _)| *key == id)
				.map(|(_, data)| data);
			bytes = bytes.saturating_add(cost(next.unwrap_or(&draft.data))?);
			if !Arc::ptr_eq(&draft.data, &draft.saved) || next.is_some() {
				bytes = bytes.saturating_add(cost(&draft.saved)?);
			}
		}
		if bytes > MAX_RETAINED_DOCUMENT_BYTES {
			return Err("Retained scene document byte budget exceeded".into());
		}
		Ok(())
	}
}
