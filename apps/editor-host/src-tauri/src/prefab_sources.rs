//! Workspace-owned source drafts. Saving compares the captured source bytes
//! with both preparation and atomic publication; instances update only explicitly.
use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind},
	editor::command::{EditorCommandDto, EditorCommandRequest},
	linked_prefab::{LinkedPrefabOverrideDto, LinkedPrefabSource, PrefabConflictResolution},
};
use serde::Serialize;

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
			.export_prefab_with_slots(reference_slots)?
			.into_component_source()?;
		let prepared =
			runtime::assets::project_assets::prepare_prefab(&self.asset_root, path, &prefab)?;
		if prepared.previous_fingerprint().is_some() {
			return Err(
				"Source destination appeared while preparing it; choose another path".into(),
			);
		}
		prepared.publish()?;
		self.open_prefab_source(path, revision)
	}
	pub fn instantiate_linked_source(
		&mut self,
		path: &str,
		draft_id: &str,
		position: [f32; 3],
		bindings: std::collections::BTreeMap<String, EntityKey>,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		let source = self.reviewed_prefab_source(draft_id, path, revision)?;
		source.validate()?;
		self.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::InstantiateLinkedPrefab {
					source,
					position,
					bindings,
				},
			})
			.map_err(|error| error.to_string())?;
		self.cache_bounds();
		self.backend.snapshot().map_err(|error| error.to_string())
	}
	fn linked_root(&self, key: &str) -> Result<Option<EntityKey>, String> {
		let key = EntityKey::new(key)?;
		let scene = self.backend.session().authored_document();
		Ok(scene
			.entities
			.iter()
			.find(|entity| {
				entity.linked_prefab.as_ref().is_some_and(|link| {
					entity.key == key || link.entity_map.values().any(|member| member == &key)
				})
			})
			.map(|entity| entity.key.clone()))
	}
	pub fn linked_prefab_status(
		&self,
		key: &str,
		revision: u64,
	) -> Result<Option<LinkedPrefabStatusDto>, String> {
		self.check_revision(revision)?;
		let Some(root) = self.linked_root(key)? else {
			return Ok(None);
		};
		let scene = self.backend.session().authored_document();
		let link = scene
			.entities
			.iter()
			.find(|entity| entity.key == root)
			.and_then(|entity| entity.linked_prefab.as_ref())
			.expect("root resolved");
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root);
		let path = catalog
			.as_ref()
			.ok()
			.and_then(|catalog| catalog.by_id(&link.source.source_id))
			.map_or_else(|| link.source.path.clone(), |asset| asset.path.clone());
		let source = self.prefab_source(&path).and_then(|source| {
			if source.source_id != link.source.source_id {
				return Err("Linked source identity is unavailable".into());
			}
			Ok(source)
		});
		let changed = source
			.as_ref()
			.is_ok_and(|source| source.fingerprint != link.source.fingerprint);
		let error = source.as_ref().err().cloned();
		Ok(Some(LinkedPrefabStatusDto {
			root_key: root.clone(),
			source_id: link.source.source_id.clone(),
			source_path: path,
			source_fingerprint: link.source.fingerprint.clone(),
			source_changed: changed,
			source_error: error,
			bindings: link.bindings.clone(),
			overrides: runtime::linked_prefab::override_status(&scene, &root)?,
		}))
	}
	pub fn update_linked_source(
		&mut self,
		key: &str,
		draft_id: &str,
		resolution: PrefabConflictResolution,
		bindings: Option<std::collections::BTreeMap<String, EntityKey>>,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.prefab_edit_context(revision)?;
		let root = self.linked_root(key)?.ok_or("Select a linked instance")?;
		let scene = self.backend.session().authored_document();
		let id = scene
			.entities
			.iter()
			.find(|entity| entity.key == root)
			.and_then(|entity| entity.linked_prefab.as_ref())
			.expect("root resolved")
			.source
			.source_id
			.clone();
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let asset = catalog
			.by_id(&id)
			.ok_or("Linked source identity is unavailable")?;
		let source = self.reviewed_prefab_source(draft_id, &asset.path, revision)?;
		drop(scene);
		self.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::UpdateLinkedPrefab {
					root_key: root.as_str().into(),
					source,
					resolution,
					bindings,
				},
			})
			.map_err(|error| error.to_string())?;
		self.cache_bounds();
		self.backend.snapshot().map_err(|error| error.to_string())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeMap;
	struct Fixture(std::path::PathBuf);
	impl Drop for Fixture {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}
	fn fixture() -> (Fixture, EditorWorkspace) {
		let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../../target")
			.join(format!("m5-prefab-source-{}", AssetId::generate().as_str()));
		for path in ["assets/props", "assets/scenes", "assets/prefabs"] {
			std::fs::create_dir_all(root.join(path)).unwrap();
		}
		std::fs::copy(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../game/assets/props/cube.glb"),
			root.join("assets/props/cube.glb"),
		)
		.unwrap();
		let mut scene =
			runtime::SceneData::from_json(include_str!("../../../game/assets/scenes/robots.json"))
				.unwrap();
		scene.entities = vec![runtime::scene_data::EntityData::new(
			EntityKey::new("prop").unwrap(),
			"Prop",
			"props/cube.glb",
		)];
		scene.bindings.clear();
		scene.camera.follow = None;
		std::fs::write(
			root.join("assets/scenes/main.json"),
			scene.to_json().unwrap(),
		)
		.unwrap();
		let mut workspace =
			EditorWorkspace::open(root.join("assets").to_str().unwrap(), "scenes/main.json")
				.unwrap();
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: EditorCommandDto::Select {
					keys: vec!["prop".into()],
				},
			})
			.unwrap();
		(Fixture(root), workspace)
	}
	#[test]
	fn reviewed_link_rejects_source_changes_before_instance_publication() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		let before = workspace.backend.session().authored_document().clone();
		let mut external = draft.prefab.clone();
		external["entities"][0]["name"] = "Unreviewed replacement".into();
		std::fs::write(
			workspace.asset_root.join("prefabs/prop.json"),
			external.to_string(),
		)
		.unwrap();
		assert!(
			workspace
				.instantiate_linked_source(
					"prefabs/prop.json",
					&draft.draft_id,
					[0.0; 3],
					BTreeMap::new(),
					revision
				)
				.is_err(),
			"a reviewed source must not silently instantiate later disk content"
		);
		assert_eq!(workspace.backend.session().revision(), revision);
		assert_eq!(workspace.backend.session().authored_document(), before);
	}
	#[test]
	fn reviewed_update_rejects_replaced_bytes_and_accepts_a_fresh_review() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&draft.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision,
			)
			.unwrap();
		workspace.close_prefab_source(&draft.draft_id).unwrap();
		let revision = workspace.backend.session().revision();
		let before = workspace.backend.session().authored_document().clone();
		let root = before
			.entities
			.iter()
			.find(|entity| entity.linked_prefab.is_some())
			.unwrap()
			.key
			.clone();
		let reviewed = workspace
			.open_prefab_source("prefabs/prop.json", revision)
			.unwrap();
		let mut external = reviewed.prefab.clone();
		external["entities"][0]["name"] = "Changed after review".into();
		std::fs::write(
			workspace.asset_root.join("prefabs/prop.json"),
			external.to_string(),
		)
		.unwrap();
		let error = workspace
			.update_linked_source(
				root.as_str(),
				&reviewed.draft_id,
				PrefabConflictResolution::UseSource,
				None,
				revision,
			)
			.err()
			.unwrap();
		assert!(error.contains("changed on disk"), "{error}");
		assert_eq!(workspace.backend.session().revision(), revision);
		assert_eq!(workspace.backend.session().authored_document(), before);
		workspace.close_prefab_source(&reviewed.draft_id).unwrap();
		let fresh = workspace
			.open_prefab_source("prefabs/prop.json", revision)
			.unwrap();
		workspace
			.update_linked_source(
				root.as_str(),
				&fresh.draft_id,
				PrefabConflictResolution::UseSource,
				None,
				revision,
			)
			.unwrap();
		assert!(workspace
			.backend
			.session()
			.authored_document()
			.entities
			.iter()
			.any(|entity| entity.name == "Changed after review"));
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: EditorCommandDto::Undo,
			})
			.unwrap();
		assert_eq!(workspace.backend.session().authored_document(), before);
	}
	#[test]
	fn reviewed_link_rejects_foreign_expired_and_replaced_identity_captures() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		let foreign = workspace
			.create_linked_prefab_source("prefabs/other.json", BTreeMap::new(), revision)
			.unwrap();
		let before = workspace.backend.session().authored_document().clone();
		assert!(workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&foreign.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision
			)
			.err()
			.unwrap()
			.contains("differs"));
		workspace.close_prefab_source(&foreign.draft_id).unwrap();
		assert!(workspace
			.instantiate_linked_source(
				"prefabs/other.json",
				&foreign.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision
			)
			.err()
			.unwrap()
			.contains("expired"));
		let catalog_path = workspace
			.asset_root
			.join(runtime::asset_catalog::CATALOG_FILE);
		let mut catalog =
			runtime::assets::project_assets::load_catalog(&workspace.asset_root).unwrap();
		catalog
			.assets
			.iter_mut()
			.find(|asset| asset.path == "prefabs/prop.json")
			.unwrap()
			.id = AssetId::generate();
		std::fs::write(catalog_path, catalog.to_json().unwrap()).unwrap();
		assert!(workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&draft.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision
			)
			.err()
			.unwrap()
			.contains("differs"));
		assert_eq!(workspace.backend.session().revision(), revision);
		assert_eq!(workspace.backend.session().authored_document(), before);
	}
	#[test]
	fn generic_host_commands_cannot_bypass_reviewed_source_authority() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		let source = workspace.prefab_source("prefabs/prop.json").unwrap();
		let error = workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::InstantiateLinkedPrefab {
					source: source.clone(),
					position: [0.0; 3],
					bindings: BTreeMap::new(),
				},
			})
			.unwrap_err();
		assert!(error.message.contains("guarded prefab"));
		assert_eq!(workspace.backend.session().revision(), revision);
		workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&draft.draft_id,
				[0.0; 3],
				BTreeMap::new(),
				revision,
			)
			.unwrap();
		let revision = workspace.backend.session().revision();
		let before = workspace.backend.session().authored_document().clone();
		let root = before
			.entities
			.iter()
			.find(|entity| entity.linked_prefab.is_some())
			.unwrap()
			.key
			.as_str()
			.to_owned();
		let error = workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::UpdateLinkedPrefab {
					root_key: root,
					source,
					resolution: PrefabConflictResolution::UseSource,
					bindings: None,
				},
			})
			.unwrap_err();
		assert!(error.message.contains("guarded prefab"));
		assert_eq!(workspace.backend.session().revision(), revision);
		assert_eq!(workspace.backend.session().authored_document(), before);
	}
	#[test]
	fn source_draft_saves_canonical_components_and_conflicts_with_external_disk_edits() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		assert!(draft.prefab["entities"][0].get("components").is_some());
		let mut value = draft.prefab.clone();
		value["entities"][0]["name"] = "Edited".into();
		workspace
			.validate_prefab_source(&draft.draft_id, value.clone(), revision)
			.unwrap();
		let saved = workspace
			.save_prefab_source(&draft.draft_id, value.clone(), revision)
			.unwrap();
		assert_ne!(saved.draft.fingerprint, draft.fingerprint);
		let path = workspace.asset_root.join("prefabs/prop.json");
		let catalog = std::fs::read(
			workspace
				.asset_root
				.join(runtime::asset_catalog::CATALOG_FILE),
		)
		.unwrap();
		value["entities"][0]["name"] = "Extern".into();
		let external = serde_json::to_vec(&value).unwrap();
		std::fs::write(&path, &external).unwrap();
		assert!(workspace
			.save_prefab_source(&draft.draft_id, saved.draft.prefab.clone(), revision)
			.err()
			.unwrap()
			.contains("changed on disk"));
		assert_eq!(std::fs::read(path).unwrap(), external);
		assert_eq!(
			std::fs::read(
				workspace
					.asset_root
					.join(runtime::asset_catalog::CATALOG_FILE)
			)
			.unwrap(),
			catalog
		);
		workspace.close_prefab_source(&draft.draft_id).unwrap();
		assert!(workspace
			.validate_prefab_source(&draft.draft_id, value, revision)
			.is_err());
	}
	#[test]
	fn source_updates_are_explicit_undoable_and_missing_sources_do_not_break_pinned_play() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		workspace
			.instantiate_linked_source(
				"prefabs/prop.json",
				&draft.draft_id,
				[2.0, 0.0, 0.0],
				BTreeMap::new(),
				revision,
			)
			.unwrap();
		let revision = workspace.backend.session().revision();
		let scene = workspace.backend.session().authored_document();
		let anchor = scene
			.entities
			.iter()
			.find(|entity| entity.linked_prefab.is_some())
			.unwrap();
		let root = anchor.key.clone();
		let member = anchor
			.linked_prefab
			.as_ref()
			.unwrap()
			.entity_map
			.values()
			.next()
			.unwrap()
			.clone();
		drop(scene);
		let opened = workspace
			.open_prefab_source("prefabs/prop.json", revision)
			.unwrap();
		let mut value = opened.prefab;
		value["entities"][0]["name"] = "New source name".into();
		workspace
			.save_prefab_source(&opened.draft_id, value, revision)
			.unwrap();
		assert_eq!(
			workspace
				.backend
				.session()
				.authored_document()
				.entities
				.iter()
				.find(|entity| entity.key == member)
				.unwrap()
				.name,
			"Prop"
		);
		assert!(
			workspace
				.linked_prefab_status(member.as_str(), revision)
				.unwrap()
				.unwrap()
				.source_changed
		);
		workspace
			.update_linked_source(
				root.as_str(),
				&opened.draft_id,
				PrefabConflictResolution::Reject,
				None,
				revision,
			)
			.unwrap();
		assert_eq!(
			workspace
				.backend
				.session()
				.authored_document()
				.entities
				.iter()
				.find(|entity| entity.key == member)
				.unwrap()
				.name,
			"New source name"
		);
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: EditorCommandDto::Undo,
			})
			.unwrap();
		assert_eq!(
			workspace
				.backend
				.session()
				.authored_document()
				.entities
				.iter()
				.find(|entity| entity.key == member)
				.unwrap()
				.name,
			"Prop"
		);
		std::fs::remove_file(workspace.asset_root.join("prefabs/prop.json")).unwrap();
		let revision = workspace.backend.session().revision();
		assert!(workspace
			.linked_prefab_status(root.as_str(), revision)
			.unwrap()
			.unwrap()
			.source_error
			.is_some());
		let before = workspace.backend.snapshot().unwrap();
		assert!(workspace
			.update_linked_source(
				root.as_str(),
				&opened.draft_id,
				PrefabConflictResolution::Reject,
				None,
				revision
			)
			.is_err());
		assert_eq!(workspace.backend.session().revision(), before.revision);
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: revision,
				command: EditorCommandDto::StartPlay,
			})
			.unwrap();
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command: EditorCommandDto::StopPlay,
			})
			.unwrap();
		assert!(workspace
			.backend
			.session()
			.authored_document()
			.entities
			.iter()
			.any(|entity| entity.linked_prefab.is_some()));
		assert!(workspace
			.validate_prefab_source(
				&draft.draft_id,
				draft.prefab,
				workspace.backend.session().revision()
			)
			.is_err());
	}
	#[test]
	fn drafts_reject_other_generation_and_component_sources_cannot_overwrite_scenes() {
		let (_fixture, mut workspace) = fixture();
		let revision = workspace.backend.session().revision();
		assert!(workspace
			.create_linked_prefab_source("scenes/main.json", BTreeMap::new(), revision)
			.is_err());
		let draft = workspace
			.create_linked_prefab_source("prefabs/prop.json", BTreeMap::new(), revision)
			.unwrap();
		workspace.generation += 1;
		assert!(workspace
			.save_prefab_source(&draft.draft_id, draft.prefab, revision)
			.err()
			.unwrap()
			.contains("generation"));
	}
	#[test]
	fn component_source_creation_declares_external_actor_slot_and_link_binds_it_explicitly() {
		use runtime::{
			components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
			scene_data::{ColliderData, ColliderInteraction},
		};
		let (_fixture, mut workspace) = fixture();
		let mut scene = (*workspace.backend.session().authored_document()).clone();
		let actor = EntityKey::new("actor").unwrap();
		let door = EntityKey::new("door").unwrap();
		let trigger = EntityKey::new("trigger").unwrap();
		let solid = ColliderData::Sphere {
			radius: 0.2,
			offset: [0.0; 3],
			interaction: ColliderInteraction::default(),
		};
		let sensor = ColliderData::Sphere {
			radius: 0.2,
			offset: [0.0; 3],
			interaction: ColliderInteraction {
				sensor: true,
				..Default::default()
			},
		};
		let mut door_entity =
			runtime::scene_data::EntityData::new(door.clone(), "Door", "props/cube.glb")
				.with_collider(Some(solid.clone()));
		door_entity.native_behaviors.push(NativeBehaviorComponent {
			id: ComponentId::new("key-door").unwrap(),
			enabled: true,
			behavior: NativeBehaviorSpec::KeyDoor {
				actor: actor.clone(),
				key_trigger: trigger.clone(),
				door: door.clone(),
				key_visual: None,
			},
		});
		scene.entities = vec![
			runtime::scene_data::EntityData::new(actor.clone(), "Actor", "props/cube.glb")
				.with_collider(Some(solid)),
			door_entity,
			runtime::scene_data::EntityData::new(trigger.clone(), "Trigger", "props/cube.glb")
				.with_collider(Some(sensor)),
		];
		let project =
			runtime::project::ProjectData::for_scene("Slots", "scenes/main.json", &scene).unwrap();
		workspace.backend = runtime::Engine::new(scene, project).unwrap();
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: 0,
				command: EditorCommandDto::Select {
					keys: vec![door.as_str().into(), trigger.as_str().into()],
				},
			})
			.unwrap();
		let revision = workspace.backend.session().revision();
		assert!(workspace
			.create_linked_prefab_source("prefabs/unbound.json", BTreeMap::new(), revision)
			.is_err());
		let draft = workspace
			.create_linked_prefab_source(
				"prefabs/door.json",
				BTreeMap::from([("actor".into(), actor.clone())]),
				revision,
			)
			.unwrap();
		assert_eq!(draft.prefab["reference_slots"]["actor"], actor.as_str());
		assert!(workspace
			.instantiate_linked_source(
				"prefabs/door.json",
				&draft.draft_id,
				[1.0, 0.0, 0.0],
				BTreeMap::new(),
				revision
			)
			.is_err());
		workspace
			.instantiate_linked_source(
				"prefabs/door.json",
				&draft.draft_id,
				[1.0, 0.0, 0.0],
				BTreeMap::from([("actor".into(), actor)]),
				revision,
			)
			.unwrap();
		workspace
			.backend
			.session()
			.authored_document()
			.validate()
			.unwrap();
	}
}
