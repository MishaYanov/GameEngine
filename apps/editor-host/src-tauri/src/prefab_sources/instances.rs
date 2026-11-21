use super::*;

impl EditorWorkspace {
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
