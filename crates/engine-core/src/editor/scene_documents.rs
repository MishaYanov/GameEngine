//! Retained authored documents. Filesystem preparation remains with the host.
use super::*;
use crate::{
	command::SceneStateDto,
	scene_catalog::{MAX_PROJECT_SCENES, SceneDescriptor},
};
use std::sync::Weak;

pub const MAX_RETAINED_DOCUMENT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct SceneDraft {
	pub data: Arc<SceneData>,
	pub saved: Arc<SceneData>,
	pub saved_path: Option<String>,
	pub selection: Vec<EntityKey>,
}

#[derive(Default)]
pub(super) struct DocumentCostCache {
	entries: HashMap<usize, CachedDocumentCost>,
}

struct CachedDocumentCost {
	// A weak reference protects pointer identity without retaining the document.
	document: Weak<SceneData>,
	bytes: usize,
	used: bool,
}

impl DocumentCostCache {
	fn cost(&mut self, data: &Arc<SceneData>) -> Result<usize, String> {
		let identity = Arc::as_ptr(data) as usize;
		if let Some(entry) = self.entries.get_mut(&identity) {
			entry.used = true;
			return Ok(entry.bytes);
		}
		let bytes = document_cost(data)?;
		self.entries.insert(
			identity,
			CachedDocumentCost {
				document: Arc::downgrade(data),
				bytes,
				used: true,
			},
		);
		Ok(bytes)
	}
}

fn document_cost(data: &SceneData) -> Result<usize, String> {
	Ok(data
		.serialized_size()?
		.saturating_mul(4)
		.saturating_add(std::mem::size_of::<SceneData>()))
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
		Ok(())
	}

	fn prune_retired_documents(&mut self) {
		let retained = self.history.retained_scene_ids();
		self.drafts
			.retain(|id, _| self.project.scenes.get(id).is_ok() || retained.contains(id));
	}

	pub(super) fn check_document_budget(
		&mut self,
		replacement: Option<(&SceneId, &SceneData)>,
	) -> Result<(), String> {
		Self::document_budget(
			&self.active_scene_id,
			&self.data,
			&self.saved,
			&self.drafts,
			replacement,
			&mut self.document_costs,
		)
		.map(|_| ())
	}

	pub(super) fn document_budget(
		active: &SceneId,
		data: &Arc<SceneData>,
		saved: &Arc<SceneData>,
		drafts: &HashMap<SceneId, SceneDraft>,
		replacement: Option<(&SceneId, &SceneData)>,
		cache: &mut DocumentCostCache,
	) -> Result<usize, String> {
		if drafts.len() + 1 > MAX_PROJECT_SCENES {
			return Err("Retained scene count limit exceeded".into());
		}
		for entry in cache.entries.values_mut() {
			entry.used = false;
		}
		let result = (|| {
			// Conservative logical accounting, with the same current/saved sharing
			// rules as before. Mutable replacement candidates are never cached.
			let mut bytes = if let Some((_, next)) = replacement.filter(|(id, _)| *id == active) {
				document_cost(next)?
			} else {
				cache.cost(data)?
			};
			if !Arc::ptr_eq(data, saved) || replacement.is_some_and(|(id, _)| id == active) {
				bytes = bytes.saturating_add(cache.cost(saved)?);
			}
			for (id, draft) in drafts {
				let next = replacement
					.filter(|(key, _)| *key == id)
					.map(|(_, data)| data);
				bytes = bytes.saturating_add(match next {
					Some(next) => document_cost(next)?,
					None => cache.cost(&draft.data)?,
				});
				if !Arc::ptr_eq(&draft.data, &draft.saved) || next.is_some() {
					bytes = bytes.saturating_add(cache.cost(&draft.saved)?);
				}
			}
			if bytes > MAX_RETAINED_DOCUMENT_BYTES {
				return Err("Retained scene document byte budget exceeded".into());
			}
			Ok(bytes)
		})();
		// Also prune after errors: rejected candidates and externally held retired
		// snapshots must not make this cache grow beyond retained document count.
		cache
			.entries
			.retain(|_, entry| entry.used && entry.document.strong_count() != 0);
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData};

	fn document(name: &str) -> SceneData {
		let camera = crate::Camera::default();
		SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: Default::default(),
			entities: vec![EntityData::new(
				EntityKey::new("actor").unwrap(),
				name,
				"models/actor.glb",
			)],
			camera: CameraData {
				position: camera.position,
				yaw: camera.yaw,
				pitch: camera.pitch,
				vertical_fov: camera.vertical_fov,
				near_plane: camera.near,
				far_plane: camera.far,
			},
			lights: vec![],
		}
	}

	fn serialized_cost(data: &SceneData) -> usize {
		data.to_json().unwrap().len() * 4 + std::mem::size_of::<SceneData>()
	}

	#[test]
	fn counted_cost_matches_pretty_json_and_validation_errors() {
		let mut data = document("Actör \"quoted\" \\ name");
		data.bindings
			.insert("主角".into(), EntityKey::new("actor").unwrap());
		data.entities[0].transform.position = [-0.0, 1.0e-5, 1.0e12];
		assert_eq!(document_cost(&data).unwrap(), serialized_cost(&data));
		data.schema_version = 0;
		assert_eq!(document_cost(&data), data.to_json().map(|_| 0));
		let mut cache = DocumentCostCache::default();
		assert!(cache.cost(&Arc::new(data)).is_err());
		assert!(cache.entries.is_empty());
	}

	#[test]
	fn budget_counts_current_saved_and_replacement_documents_exactly() {
		let active = SceneId::new("main").unwrap();
		let second = SceneId::new("second").unwrap();
		let data = Arc::new(document("active"));
		let inactive = Arc::new(document("inactive"));
		let mut replacement = document("replacement has a longer name");
		let drafts = HashMap::from([(
			second.clone(),
			SceneDraft {
				data: Arc::clone(&inactive),
				saved: Arc::clone(&inactive),
				saved_path: Some("scenes/second.json".into()),
				selection: vec![],
			},
		)]);
		let mut cache = DocumentCostCache::default();
		let shared = serialized_cost(&data) + serialized_cost(&inactive);
		assert_eq!(
			EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
				.unwrap(),
			shared
		);
		for id in [&active, &second] {
			assert_eq!(
				EditorSession::document_budget(
					&active,
					&data,
					&data,
					&drafts,
					Some((id, &replacement)),
					&mut cache,
				)
				.unwrap(),
				shared + serialized_cost(&replacement)
			);
		}
		replacement.entities[0].name.push_str(" changed again");
		assert_eq!(
			EditorSession::document_budget(
				&active,
				&data,
				&data,
				&drafts,
				Some((&active, &replacement)),
				&mut cache,
			)
			.unwrap(),
			shared + serialized_cost(&replacement)
		);
		let separate_saved = Arc::new((*data).clone());
		assert_eq!(
			EditorSession::document_budget(
				&active,
				&data,
				&separate_saved,
				&drafts,
				None,
				&mut cache,
			)
			.unwrap(),
			shared + serialized_cost(&separate_saved)
		);
	}

	#[test]
	fn cost_cache_replaces_identities_and_prunes_externally_held_retired_documents() {
		let active = SceneId::new("main").unwrap();
		let mut data = Arc::new(document("before"));
		let retired = Arc::clone(&data);
		let mut cache = DocumentCostCache::default();
		let drafts = HashMap::new();
		EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache).unwrap();
		assert_eq!(Arc::strong_count(&data), 2);
		assert_eq!(cache.entries.len(), 1);
		Arc::make_mut(&mut data).entities[0].name = "a substantially longer replacement".into();
		assert_ne!(serialized_cost(&data), serialized_cost(&retired));
		assert_eq!(
			EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
				.unwrap(),
			serialized_cost(&data)
		);
		assert_eq!(cache.entries.len(), 1);
		assert!(
			!cache
				.entries
				.contains_key(&(Arc::as_ptr(&retired) as usize))
		);
		assert_eq!(Arc::strong_count(&retired), 1);
		assert_eq!(Arc::strong_count(&data), 1);
		let weak = Arc::downgrade(&data);
		drop(data);
		assert!(
			weak.upgrade().is_none(),
			"cache must not retain document contents"
		);
	}

	#[test]
	fn retained_budget_still_rejects_the_next_inactive_document_after_cached_counting() {
		let active = SceneId::new("main").unwrap();
		let mut data = document("large");
		data.entities = (0..2048)
			.map(|index| {
				EntityData::new(
					EntityKey::new(format!("actor-{index}")).unwrap(),
					"x".repeat(1024),
					"models/actor.glb",
				)
			})
			.collect();
		let data = Arc::new(data);
		let cost = serialized_cost(&data);
		let allowed_count = MAX_RETAINED_DOCUMENT_BYTES / cost;
		assert!((2..MAX_PROJECT_SCENES).contains(&allowed_count));
		let mut drafts: HashMap<_, _> = (1..allowed_count)
			.map(|index| {
				(
					SceneId::new(format!("scene-{index}")).unwrap(),
					SceneDraft {
						data: Arc::clone(&data),
						saved: Arc::clone(&data),
						saved_path: Some(format!("scenes/{index}.json")),
						selection: vec![],
					},
				)
			})
			.collect();
		let mut cache = DocumentCostCache::default();
		assert_eq!(
			EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
				.unwrap(),
			allowed_count * cost
		);
		let extra = SceneId::new("extra").unwrap();
		drafts.insert(extra.clone(), drafts.values().next().unwrap().clone());
		assert_eq!(
			EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
				.unwrap_err(),
			"Retained scene document byte budget exceeded"
		);
		drafts.remove(&extra);
		assert_eq!(
			EditorSession::document_budget(&active, &data, &data, &drafts, None, &mut cache)
				.unwrap(),
			allowed_count * cost
		);
	}

	#[test]
	fn failed_post_preview_restore_keeps_document_and_history_for_retry() {
		let active = SceneId::new("main").unwrap();
		let second = SceneId::new("second").unwrap();
		let data = document("before");
		let mut project =
			ProjectData::for_scene_with_id("Test", active.clone(), "scenes/main.json", &data)
				.unwrap();
		project
			.scenes
			.insert(SceneDescriptor::new(second.clone(), "scenes/second.json").unwrap())
			.unwrap();
		let mut session = EditorSession::for_project(data, project).unwrap();
		session
			.load_saved_scenes(vec![(second.clone(), document("inactive"))])
			.unwrap();
		session
			.execute_command(EditorCommandRequest {
				expected_revision: session.revision(),
				command: Command::Rename {
					key: "actor".into(),
					name: "after".into(),
				},
			})
			.unwrap();
		let before = session.authored_document();
		let revision = session.revision();
		let scene_revision = session.scene_revision();
		assert!(
			session
				.history
				.preview_restore(&before, session.project(), false)
				.unwrap()
				.is_some()
		);
		// Simulate a workspace invariant rejected after the active patch was prepared.
		Arc::make_mut(&mut session.drafts.get_mut(&second).unwrap().data)
			.camera
			.near_plane = -1.0;
		for _ in 0..2 {
			assert!(session.restore_history(false).is_err());
			assert!(Arc::ptr_eq(&session.data, &before));
			assert_eq!(session.revision(), revision);
			assert_eq!(session.scene_revision(), scene_revision);
			assert!(session.history.can_undo());
			assert!(!session.history.can_redo());
		}
		let draft = session.drafts.get_mut(&second).unwrap();
		draft.data = Arc::clone(&draft.saved);
		session.restore_history(false).unwrap();
		assert_eq!(session.data.entities[0].name, "before");
		assert!(!session.history.can_undo());
		assert!(session.history.can_redo());
	}
}
