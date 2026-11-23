use super::*;

#[path = "instances/recovery.rs"]
mod recovery;
#[path = "instances/review.rs"]
mod review;

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
	/// Resolve an authored member through the pinned source, even if it has no overrides.
	pub fn linked_prop_address(
		&self,
		key: &str,
		generation: u64,
		revision: u64,
	) -> Result<Option<LinkedPropAddressDto>, String> {
		if generation != self.generation {
			return Err("The Inspector subject belongs to an earlier workspace".into());
		}
		self.check_revision(revision)?;
		let key = EntityKey::new(key)?;
		let session = self.backend.session();
		let scene = session.authored_document();
		let member = scene.entities.iter().find_map(|entity| {
			let link = entity.linked_prefab.as_ref()?;
			let source = link
				.entity_map
				.iter()
				.find(|(_, placed)| *placed == &key)?
				.0;
			Some((&entity.key, &link.source, source))
		});
		let Some((root, source, member)) = member else {
			return Ok(None);
		};
		let address = if source
			.prefab
			.entities
			.iter()
			.any(|entity| &entity.key == member)
		{
			PropAddress::Object {
				placement_path: Vec::new(),
				source_entity: member.clone(),
			}
		} else {
			source
				.prefab
				.expanded_composition(Some(&source.source_id))?
				.addresses
				.remove(member)
				.ok_or("The pinned Prop member has no semantic address")?
		};
		Ok(Some(LinkedPropAddressDto {
			root_key: root.clone(),
			entity_key: key,
			scene_id: session.active_scene_id().clone(),
			address,
		}))
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
		let (path, source) = self.current_linked_source(&link.source);
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
			direct_placements: link.source.prefab.composition.placements.len(),
			bindings: link.bindings.clone(),
			overrides: runtime::linked_prefab::override_status(&scene, &root)?,
		}))
	}
	fn current_linked_source(
		&self,
		pin: &LinkedPrefabSource,
	) -> (String, Result<LinkedPrefabSource, String>) {
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root);
		let path = catalog
			.as_ref()
			.ok()
			.and_then(|catalog| catalog.by_id(&pin.source_id))
			.map_or_else(|| pin.path.clone(), |asset| asset.path.clone());
		let source = self.prefab_source(&path).and_then(|source| {
			if source.source_id != pin.source_id {
				return Err("Linked source identity is unavailable".into());
			}
			Ok(source)
		});
		(path, source)
	}
	pub fn update_linked_source(
		&mut self,
		key: &str,
		draft_id: &str,
		resolution: PrefabConflictResolution,
		bindings: Option<std::collections::BTreeMap<String, EntityKey>>,
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		let (root, source) = self.resolve_reviewed_instance_source(key, draft_id, revision)?;
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
