//! Workspace-owned source drafts. Saving compares the captured source bytes
//! with both preparation and atomic publication; instances update only explicitly.
use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind},
	editor::command::{EditorCommandDto, EditorCommandRequest},
	linked_prefab::{LinkedPrefabOverrideDto, LinkedPrefabSource, PrefabConflictResolution},
};
use serde::Serialize;

#[path = "prefab_sources/instances.rs"]
mod instances;

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
	fn source_dto(
		&self,
		id: String,
		capture: &Capture,
		prefab: &PrefabData,
	) -> Result<PrefabSourceDraftDto, String> {
		Ok(PrefabSourceDraftDto {
			draft_id: id,
			workspace_generation: self.generation,
			revision: self.backend.session().revision(),
			path: capture.path.clone(),
			asset_id: capture.source_id.clone(),
			fingerprint: capture.fingerprint.clone(),
			prefab: serde_json::from_str(&prefab.to_json()?).map_err(|error| error.to_string())?,
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
		let capture = Capture {
			source_id: source.source_id,
			path: source.path,
			fingerprint: source.fingerprint,
			revision,
			generation: self.generation,
		};
		let dto = self.source_dto(id.clone(), &capture, &source.prefab)?;
		self.prefab_source_drafts.captures.insert(id, capture);
		Ok(dto)
	}
	pub fn close_prefab_source(&mut self, id: &str) -> Result<(), String> {
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
	) -> Result<LinkedPrefabSource, String> {
		let capture = self
			.prefab_source_drafts
			.captures
			.get(draft_id)
			.filter(|capture| capture.generation == self.generation)
			.ok_or("Prop draft expired or belongs to another workspace")?;
		let source = self.prefab_source(&capture.path)?;
		if source.source_id != capture.source_id
			|| source.path != capture.path
			|| source.fingerprint != capture.fingerprint
		{
			return Err("Prop source changed after opening; reopen its draft to preview the new version".into());
		}
		Ok(source)
	}
	pub(crate) fn prop_preview_source_matches(&self, draft_id: &str, fingerprint: &str) -> bool {
		self.prefab_source_drafts
			.captures
			.get(draft_id)
			.is_some_and(|capture| {
				capture.generation == self.generation && capture.fingerprint == fingerprint
			})
	}
	pub fn validate_prefab_source(
		&self,
		id: &str,
		value: serde_json::Value,
		revision: u64,
	) -> Result<PrefabSourceValidationDto, String> {
		self.source_draft(id, revision)?;
		let prefab = PrefabData::from_json(&value.to_string())?;
		let bytes = prefab.to_json()?.len();
		Ok(PrefabSourceValidationDto {
			draft_id: id.into(),
			entity_count: prefab.entities.len(),
			bytes,
		})
	}
	pub fn save_prefab_source(
		&mut self,
		id: &str,
		value: serde_json::Value,
		revision: u64,
	) -> Result<PrefabSourceSaveDto, String> {
		let capture = self.source_draft(id, revision)?;
		self.scene_output(&capture.path, None)?;
		let prefab = PrefabData::from_json(&value.to_string())?;
		let prepared = runtime::assets::project_assets::prepare_prefab(
			&self.asset_root,
			&capture.path,
			&prefab,
		)?;
		if prepared.asset().id != capture.source_id
			|| prepared.previous_fingerprint() != Some(capture.fingerprint.as_str())
		{
			return Err("Prefab source changed on disk since this draft opened; reopen it to review the changes".into());
		}
		let snapshot = self.backend.snapshot().map_err(|error| error.to_string())?;
		let next = Capture {
			fingerprint: prepared.asset().fingerprint.clone(),
			..capture
		};
		let draft = self.source_dto(id.into(), &next, &prefab)?;
		prepared.publish()?;
		self.prefab_source_drafts.captures.insert(id.into(), next);
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
		let prepared =
			runtime::assets::project_assets::prepare_prefab(&self.asset_root, path, &prefab)?;
		if prepared.previous_fingerprint().is_some() {
			return Err(
				"Source destination appeared while preparing it; choose another path".into(),
			);
		}
		let id = AssetId::generate().as_str().to_owned();
		let asset = prepared.asset();
		let capture = Capture {
			source_id: asset.id.clone(),
			path: asset.path.clone(),
			fingerprint: asset.fingerprint.clone(),
			revision,
			generation: self.generation,
		};
		let draft = self.source_dto(id.clone(), &capture, &prefab)?;
		prepared.publish()?;
		self.prefab_source_drafts.captures.insert(id, capture);
		Ok(draft)
	}
}

#[cfg(test)]
#[path = "../tests/unit/prefab_sources.rs"]
mod tests;
