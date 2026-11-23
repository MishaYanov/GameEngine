//! Workspace-owned source drafts. Saving compares the captured source bytes
//! with both preparation and atomic publication; instances update only explicitly.
use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind},
	editor::command::{EditorCommandDto, EditorCommandRequest},
	linked_prefab::{LinkedPrefabOverrideDto, LinkedPrefabSource, PrefabConflictResolution},
};
use serde::Serialize;

#[path = "prefab_sources/draft_history.rs"]
pub(crate) mod draft_history;
#[path = "prefab_sources/instances.rs"]
mod instances;
#[path = "prefab_sources/nested.rs"]
pub(crate) mod nested;
use draft_history::DraftHistory;

#[derive(Default)]
pub(super) struct PrefabSourceDrafts {
	captures: HashMap<String, Capture>,
}
#[derive(Clone)]
struct Capture {
	source_id: AssetId,
	path: String,
	fingerprint: String,
	revision: u64,
	generation: u64,
	saved: Arc<str>,
	history: DraftHistory,
}
#[derive(Serialize)]
pub struct PrefabSourceDraftDto {
	pub draft_id: String,
	pub workspace_generation: u64,
	pub revision: u64,
	pub path: String,
	pub asset_id: AssetId,
	pub fingerprint: String,
	pub prefab: serde_json::Value,
	pub draft_revision: u64,
	pub dirty: bool,
	pub can_undo: bool,
	pub can_redo: bool,
}
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropDraftEdit {
	Apply { prefab: serde_json::Value },
	Undo,
	Redo,
	Discard,
}
#[derive(Serialize)]
pub struct PrefabSourceValidationDto {
	pub draft_id: String,
	pub entity_count: usize,
	pub bytes: usize,
}
#[derive(Serialize)]
pub struct PrefabSourceSaveDto {
	pub draft: PrefabSourceDraftDto,
	pub snapshot: EditorSnapshotDto,
}
#[derive(Serialize)]
pub struct LinkedPrefabStatusDto {
	pub root_key: EntityKey,
	pub source_id: AssetId,
	pub source_path: String,
	pub source_fingerprint: String,
	pub source_changed: bool,
	pub source_error: Option<String>,
	pub direct_placements: usize,
	pub bindings: std::collections::BTreeMap<String, EntityKey>,
	pub overrides: Vec<LinkedPrefabOverrideDto>,
}

impl EditorWorkspace {
	fn prefab_edit_context(&self, revision: u64) -> Result<(), String> {
		self.check_revision(revision)?;
		if self.backend.session().is_playing() || self.backend.session().has_transaction() {
			return Err(
				"Stop Play and finish the current edit before editing prefab sources".into(),
			);
		}
		Ok(())
	}
	fn prefab_source(&self, path: &str) -> Result<LinkedPrefabSource, String> {
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let asset = catalog.by_path(path).ok_or(
			"Prefab source is not registered; refresh Assets or create a component source first",
		)?;
		if asset.kind != AssetKind::Prefab {
			return Err("Selected asset is not a prefab source".into());
		}
		let path =
			runtime::assets::project_assets::asset_path(&self.asset_root, &asset.path, true)?;
		let bytes = read_bounded(&path, PrefabData::MAX_JSON_BYTES as u64)?;
		let prefab =
			PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)?;
		Ok(LinkedPrefabSource {
			source_id: asset.id.clone(),
			path: asset.path.clone(),
			fingerprint: runtime::assets::project_assets::fingerprint(&bytes),
			prefab,
		})
	}
	fn source_dto(&self, id: String, capture: &Capture) -> Result<PrefabSourceDraftDto, String> {
		Ok(PrefabSourceDraftDto {
			draft_id: id,
			workspace_generation: self.generation,
			revision: self.backend.session().revision(),
			path: capture.path.clone(),
			asset_id: capture.source_id.clone(),
			fingerprint: capture.fingerprint.clone(),
			prefab: serde_json::from_str(capture.history.document())
				.map_err(|error| error.to_string())?,
			draft_revision: capture.history.revision(),
			dirty: capture.history.document().as_ref() != capture.saved.as_ref(),
			can_undo: capture.history.can_undo(),
			can_redo: capture.history.can_redo(),
		})
	}
	pub fn open_prefab_source(
		&mut self,
		path: &str,
		revision: u64,
	) -> Result<PrefabSourceDraftDto, String> {
		self.prefab_edit_context(revision)?;
		if self.prefab_source_drafts.captures.len() >= 4 {
			return Err("Close a prefab source draft before opening another (limit 4)".into());
		}
		let source = self.prefab_source(path)?;
		self.scene_output(&source.path, None)?;
		let id = AssetId::generate().as_str().to_owned();
		let saved: Arc<str> = source.prefab.to_json()?.into();
		let capture = Capture {
			source_id: source.source_id,
			path: source.path,
			fingerprint: source.fingerprint,
			revision,
			generation: self.generation,
			saved: Arc::clone(&saved),
			history: DraftHistory::new(saved),
		};
		let dto = self.source_dto(id.clone(), &capture)?;
		self.prefab_source_drafts.captures.insert(id, capture);
		Ok(dto)
	}
	pub fn close_prefab_source(&mut self, id: &str) -> Result<(), String> {
		if self
			.prefab_source_drafts
			.captures
			.get(id)
			.is_some_and(|capture| capture.history.document().as_ref() != capture.saved.as_ref())
		{
			return Err("Save or discard the Prop changes before closing its draft".into());
		}
		self.prefab_source_drafts.captures.remove(id);
		Ok(())
	}
	fn source_draft(&self, id: &str, revision: u64) -> Result<Capture, String> {
		self.prefab_edit_context(revision)?;
		let capture = self
			.prefab_source_drafts
			.captures
			.get(id)
			.cloned()
			.ok_or("Prefab source draft expired or belongs to another workspace")?;
		if capture.generation != self.generation {
			return Err(
				"Prefab source draft belongs to an older workspace generation; reopen it".into(),
			);
		}
		if capture.revision != revision {
			return Err(
				"Authoring changed after opening this source draft; reopen it before applying it"
					.into(),
			);
		}
		Ok(capture)
	}
	fn reviewed_prefab_source(
		&self,
		id: &str,
		path: &str,
		revision: u64,
	) -> Result<LinkedPrefabSource, String> {
		let capture = self.source_draft(id, revision)?;
		let source = self.prefab_source(path)?;
		if source.source_id != capture.source_id || source.path != capture.path {
			return Err("Prefab source differs from the reviewed draft; reopen it".into());
		}
		if source.fingerprint != capture.fingerprint {
			return Err("Prefab source changed on disk since this draft opened; reopen it to review the changes".into());
		}
		Ok(source)
	}
	pub(crate) fn reviewed_prop_preview_source(
		&self,
		draft_id: &str,
	) -> Result<(LinkedPrefabSource, u64), String> {
		let capture = self
			.prefab_source_drafts
			.captures
			.get(draft_id)
			.filter(|capture| capture.generation == self.generation)
			.ok_or("Prop draft expired or belongs to another workspace")?;
		let mut source = self.prefab_source(&capture.path)?;
		if source.source_id != capture.source_id
			|| source.path != capture.path
			|| source.fingerprint != capture.fingerprint
		{
			return Err(
				"Prop source changed after opening; reopen its draft to preview the new version"
					.into(),
			);
		}
		source.prefab = PrefabData::from_json(capture.history.document())?;
		Ok((source, capture.history.revision()))
	}
	pub(crate) fn prop_preview_source_matches(
		&self,
		draft_id: &str,
		fingerprint: &str,
		draft_revision: u64,
	) -> bool {
		self.prefab_source_drafts
			.captures
			.get(draft_id)
			.is_some_and(|capture| {
				capture.generation == self.generation
					&& capture.fingerprint == fingerprint
					&& capture.history.revision() == draft_revision
			})
	}

	pub(crate) fn has_dirty_prop_sources(&self) -> bool {
		self.prefab_source_drafts
			.captures
			.values()
			.any(|capture| capture.history.document().as_ref() != capture.saved.as_ref())
	}

	/// A stable snapshot of up to four open Prop drafts, independent of scene revision.
	pub(crate) fn prop_source_draft_revisions(&self) -> Vec<(String, u64)> {
		let mut revisions: Vec<_> = self
			.prefab_source_drafts
			.captures
			.iter()
			.map(|(id, capture)| (id.clone(), capture.history.revision()))
			.collect();
		revisions.sort_unstable();
		revisions
	}

	fn editable_prop_draft(&self, id: &str, expected: u64) -> Result<&Capture, String> {
		if self.backend.session().is_playing() || self.backend.session().has_transaction() {
			return Err(
				"Stop Play and finish the pending scene edit before modifying a Prop draft".into(),
			);
		}
		let capture = self
			.prefab_source_drafts
			.captures
			.get(id)
			.filter(|capture| capture.generation == self.generation)
			.ok_or("Prop draft expired or belongs to another workspace")?;
		capture.history.check(expected)?;
		Ok(capture)
	}

	pub(crate) fn edit_prop_draft(
		&mut self,
		id: &str,
		expected: u64,
		edit: PropDraftEdit,
	) -> Result<PrefabSourceDraftDto, String> {
		let mut next = self.editable_prop_draft(id, expected)?.clone();
		match edit {
			PropDraftEdit::Apply { prefab } => {
				let prefab = PrefabData::from_json(&prefab.to_string())?;
				prefab.validate_linked_player_components()?;
				prefab.composition.validate(
					Some(&next.source_id),
					&prefab.entities,
					&prefab.reference_slots,
				)?;
				self.validate_prefab_audio_clips(&prefab)?;
				next.history.apply(expected, prefab.to_json()?.into())?;
			}
			PropDraftEdit::Undo => next.history.undo(expected)?,
			PropDraftEdit::Redo => next.history.redo(expected)?,
			PropDraftEdit::Discard => next.history.discard(expected, Arc::clone(&next.saved))?,
		}
		self.install_prop_draft(id, next)
	}

	pub(crate) fn apply_prop_preview_position(
		&mut self,
		token: &str,
		expected: u64,
		key: &EntityKey,
		position: [f32; 3],
	) -> Result<PrefabSourceDraftDto, String> {
		let id = self.prop_preview_edit_draft(token, expected)?;
		if !self.prop_preview_selection_matches(key) {
			return Err("The selected Prop object changed during the gesture".into());
		}
		let mut next = self.editable_prop_draft(&id, expected)?.clone();
		let mut prefab = PrefabData::from_json(next.history.document())?;
		let entity = prefab
			.entities
			.iter_mut()
			.find(|entity| &entity.key == key)
			.ok_or("Object does not belong to this Prop draft")?;
		entity.transform.position = position;
		next.history.apply(expected, prefab.to_json()?.into())?;
		self.install_prop_draft(&id, next)
	}

	fn install_prop_draft(
		&mut self,
		id: &str,
		next: Capture,
	) -> Result<PrefabSourceDraftDto, String> {
		let dto = self.source_dto(id.into(), &next)?;
		let refresh = self.prepare_prop_draft_preview(
			id,
			&next.fingerprint,
			next.history.revision(),
			next.history.document(),
		)?;
		self.prefab_source_drafts.captures.insert(id.into(), next);
		self.apply_prop_draft_preview(refresh);
		Ok(dto)
	}

	pub(crate) fn save_prop_draft(
		&mut self,
		id: &str,
		expected: u64,
	) -> Result<PrefabSourceSaveDto, String> {
		let next = self.editable_prop_draft(id, expected)?.clone();
		self.publish_prop_draft(id, next)
	}
	pub fn validate_prefab_source(
		&self,
		id: &str,
		value: serde_json::Value,
		revision: u64,
	) -> Result<PrefabSourceValidationDto, String> {
		self.source_draft(id, revision)?;
		let prefab = PrefabData::from_json(&value.to_string())?;
		prefab.validate_linked_player_components()?;
		self.validate_prefab_audio_clips(&prefab)?;
		let bytes = prefab.to_json()?.len();
		Ok(PrefabSourceValidationDto {
			draft_id: id.into(),
			entity_count: prefab.entities.len(),
			bytes,
		})
	}

	fn validate_prefab_audio_clips(&self, prefab: &PrefabData) -> Result<(), String> {
		let expanded = prefab.expanded_entities()?;
		let mut clips = expanded
			.iter()
			.filter_map(|entity| entity.audio_source.as_ref()?.settings.clip.as_ref())
			.peekable();
		if clips.peek().is_some() {
			let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
			for clip in clips {
				catalog.audio_clip(&clip.asset)?;
			}
		}
		Ok(())
	}
	pub fn save_prefab_source(
		&mut self,
		id: &str,
		value: serde_json::Value,
		revision: u64,
	) -> Result<PrefabSourceSaveDto, String> {
		let mut next = self.source_draft(id, revision)?;
		if next.history.document().as_ref() != next.saved.as_ref() {
			return Err("Save the accepted Prop draft from its workspace".into());
		}
		let prefab = PrefabData::from_json(&value.to_string())?;
		next.history
			.apply(next.history.revision(), prefab.to_json()?.into())?;
		self.publish_prop_draft(id, next)
	}

	fn publish_prop_draft(
		&mut self,
		id: &str,
		mut next: Capture,
	) -> Result<PrefabSourceSaveDto, String> {
		self.scene_output(&next.path, None)?;
		let prefab = PrefabData::from_json(next.history.document())?;
		prefab.validate_linked_player_components()?;
		let prepared =
			runtime::assets::project_assets::prepare_prefab(&self.asset_root, &next.path, &prefab)?;
		if prepared.asset().id != next.source_id
			|| prepared.previous_fingerprint() != Some(next.fingerprint.as_str())
		{
			return Err("Prefab source changed on disk since this draft opened; reopen it to review the changes".into());
		}
		let snapshot = self.backend.snapshot().map_err(|error| error.to_string())?;
		next.fingerprint = prepared.asset().fingerprint.clone();
		next.saved = Arc::clone(next.history.document());
		let refresh = self.prepare_prop_draft_preview(
			id,
			&next.fingerprint,
			next.history.revision(),
			next.history.document(),
		)?;
		let draft = self.source_dto(id.into(), &next)?;
		prepared.publish()?;
		self.prefab_source_drafts.captures.insert(id.into(), next);
		self.apply_prop_draft_preview(refresh);
		Ok(PrefabSourceSaveDto { draft, snapshot })
	}
	pub fn create_linked_prefab_source(
		&mut self,
		path: &str,
		reference_slots: std::collections::BTreeMap<String, EntityKey>,
		revision: u64,
	) -> Result<PrefabSourceDraftDto, String> {
		self.create_linked_prefab_source_at(path, reference_slots, None, revision)
	}

	pub fn create_linked_prefab_source_at(
		&mut self,
		path: &str,
		reference_slots: std::collections::BTreeMap<String, EntityKey>,
		pivot: Option<&EntityKey>,
		revision: u64,
	) -> Result<PrefabSourceDraftDto, String> {
		self.prefab_edit_context(revision)?;
		self.scene_output(path, None)?;
		if self.prefab_source_drafts.captures.len() >= 4 {
			return Err("Close a prefab source draft before creating another".into());
		}
		if self.asset_root.join(path).exists() {
			return Err(
				"Source destination already exists; open it as a guarded draft instead".into(),
			);
		}
		let prefab = self
			.backend
			.session()
			.export_prefab_with_slots_at(reference_slots, pivot)?
			.into_component_source()?;
		prefab.validate_linked_player_components()?;
		let prepared =
			runtime::assets::project_assets::prepare_prefab(&self.asset_root, path, &prefab)?;
		if prepared.previous_fingerprint().is_some() {
			return Err(
				"Source destination appeared while preparing it; choose another path".into(),
			);
		}
		let id = AssetId::generate().as_str().to_owned();
		let asset = prepared.asset();
		let saved: Arc<str> = prefab.to_json()?.into();
		let capture = Capture {
			source_id: asset.id.clone(),
			path: asset.path.clone(),
			fingerprint: asset.fingerprint.clone(),
			revision,
			generation: self.generation,
			saved: Arc::clone(&saved),
			history: DraftHistory::new(saved),
		};
		let draft = self.source_dto(id.clone(), &capture)?;
		prepared.publish()?;
		self.prefab_source_drafts.captures.insert(id, capture);
		Ok(draft)
	}
}

#[cfg(test)]
#[path = "../tests/unit/prefab_sources.rs"]
mod tests;
