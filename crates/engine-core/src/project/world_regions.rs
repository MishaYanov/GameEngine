//! Authored cells in one persistent simulating world.

use super::ProjectData;
use crate::{EntityKey, SceneData, scene_catalog::SceneId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_REGIONS: usize = 256;
const MAX_CELL_ENTITIES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionBounds {
	pub min: [f32; 3],
	pub max: [f32; 3],
}

impl RegionBounds {
	pub fn validate(self) -> Result<(), String> {
		if self
			.min
			.into_iter()
			.zip(self.max)
			.any(|(min, max)| !min.is_finite() || !max.is_finite() || min >= max)
		{
			return Err("Region bounds must be finite and have positive dimensions".into());
		}
		Ok(())
	}

	pub fn distance_to(self, position: [f32; 3]) -> f32 {
		let offset = std::array::from_fn(|index| {
			(self.min[index] - position[index])
				.max(0.0)
				.max(position[index] - self.max[index])
		});
		glam::Vec3::from_array(offset).length()
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldRegion {
	pub world: SceneId,
	pub scene: SceneId,
	pub bounds: RegionBounds,
	pub preload_distance: f32,
	pub release_distance: f32,
}

impl WorldRegion {
	fn validate(&self) -> Result<(), String> {
		self.bounds.validate()?;
		if !self.preload_distance.is_finite()
			|| !self.release_distance.is_finite()
			|| self.preload_distance <= 0.0
			|| self.release_distance <= self.preload_distance
		{
			return Err("Region release distance must exceed its positive preload distance".into());
		}
		Ok(())
	}

	pub fn validate_content(&self, scene: &SceneData) -> Result<(), String> {
		if scene.entities.is_empty() || scene.entities.len() > MAX_CELL_ENTITIES {
			return Err(format!(
				"Region {} must have 1–{MAX_CELL_ENTITIES} entities",
				self.scene.as_str()
			));
		}
		if !scene.bindings.is_empty()
			|| scene.default_game_camera.is_some()
			|| !scene.lights.is_empty()
			|| scene
			.entities
			.iter()
			.any(|entity| entity.camera.is_some() || entity.player_controller.is_some())
		{
			return Err(format!(
				"Region {} must leave the active camera, player, global lights, and bindings in its world",
				self.scene.as_str()
			));
		}
		self.validate_lifecycle_content(scene)
	}

	fn validate_lifecycle_content(&self, scene: &SceneData) -> Result<(), String> {
		for entity in &scene.entities {
			Self::validate_entity_key(&entity.key)?;
			if entity.character_controller.is_some()
				|| entity.animation.is_some()
				|| entity.animator.is_some()
				|| entity.sky.is_some()
				|| entity.light.is_some()
				|| entity.audio_source.is_some()
				|| entity.particle_emitter.is_some()
				|| !entity.native_behaviors.is_empty()
				|| !entity.scripts.is_empty()
			{
				return Err(format!(
					"Region {} object {:?} must be a static prop or collider; keep active gameplay and presentation components in the persistent world",
					self.scene.as_str(),
					entity.key.as_str()
				));
			}
		}
		Ok(())
	}

	pub(crate) fn validate_entity_key(key: &EntityKey) -> Result<(), String> {
		key.validate()?;
		if key.as_str().len() > 256 || key.as_str().chars().any(char::is_control) {
			return Err("Region entity key exceeds its limit".into());
		}
		Ok(())
	}
}

/// A decorative proxy in the persistent world takes over a detail mesh at
/// distance, and covers it while its containing region is unloaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLod {
	pub world: SceneId,
	pub detail: EntityKey,
	pub proxy: EntityKey,
	pub anchor: [f32; 3],
	pub detail_distance: f32,
}

impl WorldLod {
	pub fn prefer_detail(&self, eye: [f32; 3]) -> bool {
		let offset = glam::Vec3::from_array(eye) - glam::Vec3::from_array(self.anchor);
		offset.length_squared() <= self.detail_distance * self.detail_distance
	}
}

impl ProjectData {
	pub(crate) fn validate_world_lods(&self) -> Result<(), String> {
		if self.world_lods.len() > 256 {
			return Err("Project exceeds 256 world detail pairs".into());
		}
		let mut seen = HashSet::new();
		for lod in &self.world_lods {
			if self.regions_for(&lod.world).next().is_none() {
				return Err("World detail requires a registered streamed world".into());
			}
			lod.detail.validate()?;
			lod.proxy.validate()?;
			if !lod.anchor.into_iter().all(f32::is_finite)
				|| !lod.detail_distance.is_finite()
				|| lod.detail_distance <= 0.
				|| lod.detail_distance > 1.0e7
				|| !seen.insert((&lod.world, &lod.detail))
				|| !seen.insert((&lod.world, &lod.proxy))
			{
				return Err("World detail needs distinct entity keys, a finite anchor, and a positive distance".into());
			}
		}
		Ok(())
	}

	pub fn validate_world_lod_proxies(&self, id: &SceneId, data: &SceneData) -> Result<(), String> {
		for lod in self.world_lods.iter().filter(|lod| &lod.world == id) {
			if !data.entities.iter().any(|entity| {
				entity.key == lod.proxy
					&& entity.renderable
					&& entity.has_mesh_component()
					&& entity.collider.is_none()
			}) {
				return Err(format!(
					"World proxy {} must be a renderable, non-colliding mesh in the persistent world",
					lod.proxy.as_str()
				));
			}
		}
		Ok(())
	}

	pub fn regions_for(&self, world: &SceneId) -> impl Iterator<Item=&WorldRegion> {
		self.world_regions
			.iter()
			.filter(move |region| &region.world == world)
	}

	pub(crate) fn validate_world_regions(&self) -> Result<(), String> {
		if self.world_regions.len() > MAX_REGIONS {
			return Err(format!("Project exceeds {MAX_REGIONS} world regions"));
		}
		let mut cells = HashSet::new();
		for region in &self.world_regions {
			region
				.validate()
				.map_err(|error| format!("Region {}: {error}", region.scene.as_str()))?;
			self.scene_path(&region.world)?;
			self.scene_path(&region.scene)?;
			if region.scene == self.startup_scene_id || !cells.insert(&region.scene) {
				return Err(
					"Region scenes must be distinct and cannot be the startup world".into(),
				);
			}
		}
		if self
			.world_regions
			.iter()
			.any(|region| cells.contains(&region.world))
			|| self
			.scene_routes
			.iter()
			.any(|route| cells.contains(&route.from) || cells.contains(&route.to))
		{
			return Err("Streamed region scenes cannot be worlds or full scene routes".into());
		}
		Ok(())
	}

	pub fn validate_region_document(&self, id: &SceneId, scene: &SceneData) -> Result<(), String> {
		if let Some(region) = self.world_regions.iter().find(|region| &region.scene == id) {
			region.validate_content(scene)?;
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn distance_measures_the_nearest_face_in_world_coordinates() {
		let bounds = RegionBounds {
			min: [0.0; 3],
			max: [10.0; 3],
		};
		assert_eq!(bounds.distance_to([5.0; 3]), 0.0);
		assert_eq!(bounds.distance_to([13.0, 14.0, 5.0]), 5.0);
		assert!(
			RegionBounds {
				min: [0.0; 3],
				max: [0.0; 3]
			}
				.validate()
				.is_err()
		);
	}
}
