//! CPU-only inputs which can be prepared from an immutable retained scene.
use std::sync::Arc;

use engine_core::asset_registry::AssetRef;

use crate::{SceneData, scene::validate_animation_placements};

const MAX_WARM_ENTITIES: usize = 4_096;
const MAX_WARM_PATH_BYTES: usize = 2 * 1024 * 1024;

pub struct PreparedSceneReferences {
	document: Arc<SceneData>,
	assets: Vec<AssetRef>,
}

impl PreparedSceneReferences {
	pub fn build(
		document: Arc<SceneData>,
		mut cancelled: impl FnMut() -> bool,
	) -> Result<Option<Self>, String> {
		if document.entities.len() > MAX_WARM_ENTITIES || cancelled() {
			return Ok(None);
		}
		validate_layout(&document)?;
		let mut path_bytes = 0usize;
		let mut assets = Vec::with_capacity(document.entities.len());
		for entity in &document.entities {
			if cancelled() {
				return Ok(None);
			}
			path_bytes = path_bytes.saturating_add(entity.model_path.len());
			if path_bytes > MAX_WARM_PATH_BYTES {
				return Ok(None);
			}
			assets.push(reference(entity)?);
		}
		Ok(Some(Self { document, assets }))
	}

	pub fn matches(&self, document: &SceneData) -> bool {
		std::ptr::eq(self.document.as_ref(), document)
	}

	/// Candidate model references in authored order. Hidden and animated
	/// entities keep the ordinary assembly path.
	pub fn static_model_assets(&self) -> impl Iterator<Item = &AssetRef> {
		self.document
			.entities
			.iter()
			.zip(&self.assets)
			.filter_map(|(entity, asset)| {
				(entity.renderable && entity.has_mesh_component() && entity.animation.is_none())
					.then_some(asset)
			})
	}

	pub(super) fn asset(&self, index: usize) -> &AssetRef {
		&self.assets[index]
	}
}

pub(super) fn validate_layout(document: &SceneData) -> Result<(), String> {
	let placements = document.validated_world_matrices()?;
	validate_animation_placements(document, &placements)
}

pub(super) fn reference(entity: &engine_core::scene_data::EntityData) -> Result<AssetRef, String> {
	let mut reference = entity.model_reference()?;
	reference.mesh_index = Some(reference.mesh_index.unwrap_or(0));
	Ok(reference)
}
