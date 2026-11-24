//! Picking and camera-framing metadata keyed by the authored model reference.
use std::collections::HashMap;

use crate::{
	Bounds3,
	asset_catalog::{AssetId, MeshSubassetId},
	asset_registry::AssetRef,
	scene_data::EntityData,
};

#[derive(Default)]
pub(super) struct ModelBounds {
	legacy: HashMap<String, HashMap<usize, Bounds3>>,
	stable: HashMap<AssetId, HashMap<MeshSubassetId, Bounds3>>,
}

impl ModelBounds {
	pub(super) fn clear(&mut self) {
		self.legacy.clear();
		self.stable.clear();
	}

	pub(super) fn insert_legacy(&mut self, path: String, index: usize, bounds: Bounds3) {
		self.legacy.entry(path).or_default().insert(index, bounds);
	}

	pub(super) fn insert_reference(&mut self, reference: &AssetRef, bounds: Bounds3) {
		if let (Some(asset), Some(mesh)) = (&reference.asset_id, &reference.mesh_id) {
			self.stable
				.entry(asset.clone())
				.or_default()
				.insert(mesh.clone(), bounds);
		} else {
			self.insert_legacy(
				reference.path.clone(),
				reference.mesh_index.unwrap_or(0),
				bounds,
			);
		}
	}

	pub(super) fn get(&self, entity: &EntityData) -> Option<&Bounds3> {
		if let Some(mesh) = &entity.mesh_id {
			self.stable.get(entity.asset_id.as_ref()?)?.get(mesh)
		} else {
			self.legacy
				.get(&entity.model_path)?
				.get(&entity.mesh_index.unwrap_or(0))
		}
	}
}
