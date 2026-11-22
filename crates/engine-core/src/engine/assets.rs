use super::*;

impl Engine {
	pub fn assets(&self) -> &AssetRegistry {
		&self.assets
	}

	pub fn assets_mut(&mut self) -> &mut AssetRegistry {
		&mut self.assets
	}

	/// Check before publishing prepared filesystem changes while holding the host lock.
	pub fn validate_asset_invalidation(&self) -> Result<(), BackendError> {
		self.ensure_event_capacity()?;
		if self.session.is_playing() || self.session.has_transaction() {
			return Err(self.error(
				BackendErrorCode::CommandRejected,
				"Stop Play and finish the current edit before changing assets",
			));
		}
		if self.session.revision() == u64::MAX || self.session.scene_revision() == u64::MAX {
			return Err(self.error(
				BackendErrorCode::ResourceLimit,
				"Editor revision space is exhausted",
			));
		}
		Ok(())
	}

	pub fn invalidate_assets(&mut self) -> Result<CommandReceipt, BackendError> {
		self.validate_asset_invalidation()?;
		self.session
			.invalidate_assets()
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		let changes = WorldChange {
			full_resync: true,
			..WorldChange::default()
		};
		self.publish(BackendEventKind::DocumentChanged {
			changes: changes.clone(),
		})?;
		engine_logger::debug!(target: "engine_core::assets", "Asset metadata invalidated: revision={}", self.session.revision());
		Ok(CommandReceipt {
			revision: self.session.revision(),
			scene_revision: self.session.scene_revision(),
			change_cursor: self.journal.cursor(),
			changes,
			state_changed: true,
			project_changed: false,
			organization_changed: false,
			active_scene_id: self.session.active_scene_id().clone(),
			affected_scenes: self
				.session
				.scene_states()
				.into_iter()
				.map(|scene| scene.id)
				.collect(),
		})
	}

	pub fn set_model_bounds(
		&mut self,
		path: String,
		mesh: usize,
		bounds: Bounds3,
	) -> Result<(), BackendError> {
		validate_model_path(&path)
			.map_err(|message| self.error(BackendErrorCode::InvalidCommand, message))?;
		if mesh > 1_000_000 {
			return Err(self.error(
				BackendErrorCode::InvalidCommand,
				"Asset mesh index exceeds the supported limit",
			));
		}
		self.session.set_model_bounds(path, mesh, bounds);
		Ok(())
	}

	/// Stage every metadata update before authoring can commit. An invalid
	/// scene/history change cannot leave a partially populated asset registry.
	pub fn replace_entities_with_assets(
		&mut self,
		entities: Vec<EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
		metadata: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<CommandReceipt, BackendError> {
		let (assets, _) = self
			.assets
			.staged_with_batch(metadata)
			.map_err(|message| self.error(BackendErrorCode::ResourceLimit, message))?;
		let receipt = self.replace_entities(entities, selection, label)?;
		self.assets = assets;
		self.refresh_asset_bounds();
		engine_logger::debug!(target: "engine_core::assets", "Entities and asset metadata installed: revision={}", receipt.revision);
		Ok(receipt)
	}

	pub fn register_assets(
		&mut self,
		metadata: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<Vec<AssetHandle>, BackendError> {
		let handles = self
			.assets
			.register_batch(metadata)
			.map_err(|message| self.error(BackendErrorCode::ResourceLimit, message))?;
		self.refresh_asset_bounds();
		engine_logger::debug!(target: "engine_core::assets", "Asset metadata registered: handles={}", handles.len());
		Ok(handles)
	}

	pub fn clear_assets(&mut self) {
		self.assets.clear();
		self.session.clear_model_bounds();
	}

	pub(super) fn refresh_asset_bounds(&mut self) {
		self.session.clear_model_bounds();
		for (_, record) in self.assets.iter() {
			if let Some(bounds) = record.metadata.bounds {
				self.session.set_model_bounds(
					record.reference.path.clone(),
					record.reference.mesh_index.unwrap_or(0),
					bounds,
				);
			}
		}
	}
}
