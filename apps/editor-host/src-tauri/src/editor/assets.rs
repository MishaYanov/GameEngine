use super::*;

const BOUNDS_JOB_BYTES: usize = 64 * 1024 * 1024;
// Keep half of the default 256 MiB broker available for a user import.
const BOUNDS_RESERVATION_LIMIT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Default)]
pub struct ModelReferenceUsage {
	pub legacy: HashSet<usize>,
	pub stable: HashSet<runtime::asset_catalog::MeshSubassetId>,
}

pub(super) enum PendingAsset {
	Bounds(AssetRef),
	Import { position: [f32; 3], revision: u64 },
}
#[derive(Clone, serde::Serialize)]
pub struct AssetCompletion {
	pub job_id: AssetJobId,
	pub workspace_generation: u64,
	pub accepted: bool,
	pub error: Option<String>,
	pub error_code: Option<&'static str>,
	pub revision: u64,
}
#[derive(serde::Serialize)]
pub struct AssetTicket {
	pub job_id: AssetJobId,
	pub workspace_generation: u64,
}
impl AssetCompletion {
	fn byte_cost(&self) -> usize {
		512 + self.error.as_ref().map_or(0, |message| message.len() * 6)
	}
}
fn completion_message(mut message: String) -> String {
	if message.len() > 1024 {
		let mut end = 1024;
		while !message.is_char_boundary(end) {
			end -= 1;
		}
		message.truncate(end);
	}
	message.into_boxed_str().into_string()
}

impl EditorWorkspace {
	pub(super) fn cache_bounds(&mut self) {
		#[cfg(test)]
		{
			self.bounds_reference_scans += 1;
		}
		let data = self.backend.session().authored_document();
		self.bounds_references = data
			.entities
			.iter()
			.filter(|e| e.renderable)
			.filter_map(|entity| {
				let mut reference = entity.model_reference().ok()?;
				reference.mesh_index = Some(reference.mesh_index.unwrap_or(0));
				if reference.mesh_id.is_none() {
					reference.asset_id = None;
				}
				Some(reference)
			})
			.collect();
		for (id, purpose) in &self.pending {
			if let PendingAsset::Bounds(reference) = purpose {
				if !self.bounds_references.contains(reference) {
					let _ = self.jobs.cancel(*id);
				}
			}
		}
		// Cancellation releases a reservation only when its completion is drained.
		// Keep its marker until then, including if an undo restores the reference.
		self.bounds_seen.retain(|reference| {
			self.bounds_references.contains(reference)
				|| self.pending.values().any(
					|purpose| matches!(purpose, PendingAsset::Bounds(pending) if pending == reference),
				)
		});
		self.bounds_queue = self.bounds_references.iter().cloned().collect();
		self.refill_bounds();
	}
	pub(super) fn refill_bounds(&mut self) {
		let Some(stats) = self.jobs.try_stats() else {
			return;
		};
		// Include undrained cancelled/old-generation reservations, not just the
		// current pending map. Foreground imports retain the full broker budget.
		let mut available = BOUNDS_RESERVATION_LIMIT_BYTES.saturating_sub(stats.reserved_bytes);
		while let Some(reference) = self.bounds_queue.pop_front() {
			if self.bounds_seen.contains(&reference)
				|| self.backend.assets().find(&reference).is_some()
			{
				continue;
			}
			if available >= BOUNDS_JOB_BYTES {
				if let Ok(id) = self.jobs.try_submit(reference.clone(), BOUNDS_JOB_BYTES) {
					self.pending
						.insert(id, PendingAsset::Bounds(reference.clone()));
					self.bounds_seen.insert(reference);
					available -= BOUNDS_JOB_BYTES;
					continue;
				}
			}
			// Capacity is released when completed jobs are drained. Keep the
			// remaining references without rescanning the authored document.
			self.bounds_queue.push_front(reference);
			break;
		}
	}
	pub fn submit_import(
		&mut self,
		path: String,
		position: [f32; 3],
		revision: u64,
		reserved_bytes: usize,
	) -> Result<AssetTicket, String> {
		self.check_revision(revision)?;
		if self.backend.session().is_playing() || self.backend.session().has_transaction() {
			return Err("Stop Play and finish the edit before importing".into());
		}
		if position.iter().any(|v| !v.is_finite()) {
			return Err("Import position must be finite".into());
		}
		resolve_existing(&self.asset_root, &path)?;
		let id = self
			.jobs
			.try_submit(AssetRef::new(path, None)?, reserved_bytes)
			.map_err(|e| e.to_string())?;
		self.pending
			.insert(id, PendingAsset::Import { position, revision });
		Ok(AssetTicket {
			job_id: id,
			workspace_generation: self.generation,
		})
	}
	pub fn cancel_import(&self, id: AssetJobId, generation: u64) -> Result<bool, String> {
		if generation != self.generation {
			return Err("Asset job belongs to another workspace generation".into());
		}
		self.jobs.cancel(id).map_err(|e| e.to_string())
	}
	pub fn recent_completions(&self) -> Vec<AssetCompletion> {
		self.completions.iter().cloned().collect()
	}
	pub fn drain_asset_jobs(&mut self) -> Vec<AssetCompletion> {
		let mut completions = Vec::new();
		while let Some(mut completed) = self.jobs.try_drain() {
			let Some(purpose) = self.pending.remove(&completed.id) else {
				continue;
			};
			if let PendingAsset::Bounds(reference) = &purpose {
				if !self.bounds_references.contains(reference) {
					self.bounds_seen.remove(reference);
					completed.result = Err(AssetJobError::Cancelled);
				} else if matches!(completed.result, Err(AssetJobError::Cancelled)) {
					self.bounds_seen.remove(reference);
					self.bounds_queue.push_back(reference.clone());
				}
			}
			if matches!(&purpose, PendingAsset::Import { .. })
				&& matches!(&completed.result, Err(AssetJobError::Import(_)))
			{
				engine_logger::error!(target: "editor::asset_import", "Asset import failed: job={}, workspace_generation={}", completed.id.value(), self.generation);
			}
			let result = completed
				.result
				.map_err(|error| match error {
					AssetJobError::Cancelled => ("cancelled", "Asset import cancelled".to_owned()),
					AssetJobError::Import(message) => ("import_failed", message),
					// The process panic hook already recorded this fault.
					AssetJobError::Panicked => {
						("import_failed", "CPU asset importer panicked".into())
					}
					AssetJobError::ByteLimit { .. } => (
						"byte_limit",
						"Asset import exceeded its byte reservation".to_owned(),
					),
				})
				.and_then(|asset| {
					if let PendingAsset::Import { position, revision } = purpose {
						self.check_revision(revision)
							.map_err(|message| ("conflict", message))?;
						if self.backend.session().is_playing()
							|| self.backend.session().has_transaction()
						{
							return Err((
								"invalid_state",
								"Stop Play and finish the edit before adopting an import"
									.to_owned(),
							));
						}
						self.adopt_import(&completed.reference.path, &asset.value, position)
							.map_err(|message| ("adoption_rejected", message))?;
					} else if let PendingAsset::Bounds(reference) = purpose {
						self.backend
							.register_assets(vec![(reference, asset.metadata)])
							.map_err(|error| ("metadata_rejected", error.to_string()))?;
					}
					Ok(())
				});
			let (error_code, error) = match result {
				Ok(()) => (None, None),
				Err((code, message)) => (Some(code), Some(completion_message(message))),
			};
			let completion = AssetCompletion {
				job_id: completed.id,
				workspace_generation: self.generation,
				accepted: error.is_none(),
				error,
				error_code,
				revision: self.backend.session().revision(),
			};
			engine_logger::debug!(target:"editor","Asset job completed: generation={}, job={}, accepted={}",self.generation,completion.job_id.value(),completion.accepted);
			let cost = completion.byte_cost();
			while self.completions.len() >= 64
				|| self.completion_bytes.saturating_add(cost) > 128 * 1024
			{
				if let Some(old) = self.completions.pop_front() {
					self.completion_bytes -= old.byte_cost();
				} else {
					break;
				}
			}
			self.completion_bytes += cost;
			self.completions.push_back(completion.clone());
			completions.push(completion);
		}
		self.refill_bounds();
		completions
	}
	fn imported_metadata(
		path: &str,
		imported: &ImportedScene,
	) -> Result<Vec<(AssetRef, AssetMetadata)>, String> {
		let count = imported.meshes.len().max(1) as u64;
		imported
			.meshes
			.iter()
			.map(|mesh| {
				let bounds = imported
					.mesh_bounds
					.get(mesh.mesh_index)
					.ok_or("Imported mesh bounds are missing")?;
				Ok((
					AssetRef::new(path.to_owned(), Some(mesh.mesh_index))?,
					AssetMetadata {
						bounds: *bounds,
						estimated_bytes: imported.decoded_mesh_bytes as u64 / count,
					},
				))
			})
			.collect()
	}
	pub fn used_model_references(
		&self,
		id: &runtime::asset_catalog::AssetId,
	) -> Result<ModelReferenceUsage, String> {
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let asset = catalog.by_id(id).ok_or("Asset identity is unavailable")?;
		let model_paths: HashSet<_> = std::iter::once(&asset.path)
			.chain(&asset.aliases)
			.map(|path| path.to_lowercase())
			.collect();
		let mut usage = ModelReferenceUsage::default();
		let mut record = |entity: &EntityData| {
			if let Some(entity_asset) = &entity.asset_id {
				if entity_asset != id {
					return;
				}
			} else if !model_paths.contains(&entity.model_path.to_lowercase()) {
				return;
			}
			if let Some(mesh_id) = &entity.mesh_id {
				usage.stable.insert(mesh_id.clone());
			} else {
				usage.legacy.insert(entity.mesh_index.unwrap_or(0));
			}
		};
		self.backend
			.session()
			.try_for_each_retained_entity(|entity| {
				record(entity);
				// A placement override can hide the source mesh until Revert.
				if let Some(link) = &entity.linked_prefab {
					link.source
						.prefab
						.expanded_entities()?
						.iter()
						.for_each(&mut record);
				}
				Ok(())
			})?;
		for asset in catalog
			.assets
			.iter()
			.filter(|asset| asset.kind == runtime::asset_catalog::AssetKind::Prefab)
		{
			let path = resolve_existing(&self.asset_root, &asset.path)?;
			let bytes = read_bounded(&path, PrefabData::MAX_JSON_BYTES as u64)?;
			let prefab =
				PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
			for entity in prefab.expanded_entities()?.iter() {
				record(entity);
			}
		}
		Ok(usage)
	}
	pub fn asset_published(&mut self) -> Result<EditorSnapshotDto, String> {
		self.scene_warming.invalidate_assets();
		self.metadata.clear();
		if let Err(error) = self.jobs.advance_generation() {
			engine_logger::error!(target: "editor", "Asset job generation reset failed after publication: {error}");
		}
		self.pending.clear();
		self.bounds_seen.clear();
		self.bounds_references.clear();
		self.bounds_queue.clear();
		self.backend
			.invalidate_assets()
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		self.backend.snapshot().map_err(|e| e.to_string())
	}

	pub fn import_model(
		&mut self,
		path: &str,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		if !position.iter().all(|v| v.is_finite()) {
			return Err("Import position must be finite".into());
		}
		let reference = runtime::assets::project_assets::resolve_asset(
			&self.asset_root,
			&AssetRef::new(path, None)?,
		)?;
		let path = reference.path.as_str();
		resolve_existing(&self.asset_root, path)?;
		let imported = file_operation(
			"import_model",
			self.metadata.load_scene(path).map_err(|e| e.to_string()),
		)?;
		self.adopt_import(path, &imported, position)?;
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	fn adopt_import(
		&mut self,
		path: &str,
		imported: &ImportedScene,
		position: [f32; 3],
	) -> Result<(), String> {
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let model = catalog.by_path(path);
		let asset_id = model.map(|asset| asset.id.clone());
		let mesh_ids: std::collections::HashMap<_, _> = model
			.into_iter()
			.flat_map(|asset| &asset.meshes)
			.filter_map(|mesh| Some((mesh.index, &mesh.identity.as_ref()?.id)))
			.collect();
		let remap: std::collections::HashMap<_, _> = imported
			.nodes
			.iter()
			.map(|node| (node.node_index, EntityKey::generate()))
			.collect();
		let mut entities = Vec::with_capacity(imported.nodes.len());
		let mut selection = Vec::new();
		for node in &imported.nodes {
			let key = remap
				.get(&node.node_index)
				.ok_or("Missing imported node identity")?
				.clone();
			let mut transform = node.local_transform;
			let parent = node
				.parent_node_index
				.map(|index| remap.get(&index).cloned().ok_or("Missing imported parent"))
				.transpose()?;
			if parent.is_none() {
				for (value, offset) in transform.position.iter_mut().zip(position) {
					*value += offset;
				}
				selection.push(key.clone());
			}
			let mut entity = EntityData::new(key, node.name.clone(), path)
				.with_asset_id(asset_id.clone())
				.with_renderable(node.mesh_index.is_some());
			entity.parent = parent;
			entity.mesh_index = node.mesh_index;
			entity.mesh_id = node
				.mesh_index
				.and_then(|index| mesh_ids.get(&index).map(|id| (*id).clone()));
			entity.transform = runtime::scene_data::TransformData {
				position: transform.position,
				rotation: transform.rotation,
				scale: transform.scale,
			};
			entities.push(entity);
		}
		let mut metadata = Self::imported_metadata(path, imported)?;
		for (reference, _) in &mut metadata {
			if let Some(mesh) = reference.mesh_index.and_then(|index| mesh_ids.get(&index)) {
				reference.asset_id.clone_from(&asset_id);
				reference.mesh_id = Some((*mesh).clone());
			}
		}
		self.backend
			.replace_entities_with_assets(entities, selection, "Import glTF scene", metadata)
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		engine_logger::info!(target: "editor", "Model scene imported: generation={}, nodes={}, meshes={}", self.generation, imported.nodes.len(), imported.mesh_bounds.len());
		Ok(())
	}
	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let count = prefab.entities.len();
		self.backend
			.instantiate_prefab(prefab, position)
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		engine_logger::info!(target: "editor", "Prefab instantiated: generation={}, entities={count}", self.generation);
		self.backend.snapshot().map_err(|e| e.to_string())
	}
	pub fn save_prefab(&self, path: &str, revision: u64) -> Result<String, String> {
		self.check_revision(revision)?;
		let prefab = self.backend.session().export_prefab()?;
		let output = self.scene_output(path, None)?;
		if output.extension().and_then(|e| e.to_str()) != Some("json") {
			return Err("Prefab files use .json".into());
		}
		let prepared =
			runtime::assets::project_assets::prepare_prefab(&self.asset_root, path, &prefab)?;
		file_operation("save_prefab", prepared.publish().map(|_| ()))?;
		engine_logger::info!(target: "editor", "Prefab saved: generation={}, entities={}", self.generation, prefab.entities.len());
		Ok(path.into())
	}
	pub fn load_prefab(
		&mut self,
		path: &str,
		position: [f32; 3],
		revision: u64,
	) -> Result<EditorSnapshotDto, String> {
		self.check_revision(revision)?;
		let catalog = runtime::assets::project_assets::load_catalog(&self.asset_root)?;
		let path = catalog
			.by_path(path)
			.map_or(path, |asset| asset.path.as_str());
		let path = resolve_existing(&self.asset_root, path)?;
		let bytes = file_operation("read_prefab", read_bounded(&path, 16 * 1024 * 1024))?;
		let prefab =
			PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
		let count = prefab.entities.len();
		self.backend
			.instantiate_prefab(prefab, position)
			.map_err(|e| e.to_string())?;
		self.cache_bounds();
		engine_logger::info!(target: "editor", "Prefab loaded: generation={}, entities={count}", self.generation);
		self.backend.snapshot().map_err(|e| e.to_string())
	}
}
