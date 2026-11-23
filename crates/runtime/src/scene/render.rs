use renderer::vulkan::{ModelPushConstants, RenderObject};

use super::{EntityId, MAX_SCENE_RENDER_OBJECTS, Scene, validate_animation_placement};
use crate::scene_data::LightKind;

impl Scene {
	pub fn render_objects(&self) -> Vec<RenderObject<'_>> {
		self.try_render_objects()
			.expect("Scene exceeds its rendering budget or has an invalid hierarchy")
	}

	pub fn try_render_objects(&self) -> Result<Vec<RenderObject<'_>>, String> {
		self.try_render_visible(None).map(|(objects, _)| objects)
	}

	pub fn render_visible(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
	) -> (Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats) {
		self.try_render_visible(frustum)
			.expect("Scene exceeds its rendering budget or has an invalid hierarchy")
	}

	/// Fallible rendering entry point for production callers. Check the entire
	/// scene's part count before growing a submission vector, even with culling.
	pub fn try_render_visible(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
	) -> Result<(Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats), String> {
		let mut part_count = 0usize;
		for entry in self.entries.iter().filter(|entry| entry.renderable) {
			part_count = part_count
				.checked_add(entry.instance.model().parts().len())
				.ok_or("Scene render-object count overflow")?;
			if part_count > MAX_SCENE_RENDER_OBJECTS {
				return Err(format!(
					"Scene requires more than {MAX_SCENE_RENDER_OBJECTS} render objects"
				));
			}
		}
		let mut objects = Vec::new();
		let mut stats = crate::visibility::SceneRenderStats::default();

		let world_matrices = self.cached_world_matrices()?;
		for (entry, matrix) in self.entries.iter().zip(world_matrices.iter().copied()) {
			let matrix = entry
				.key
				.as_ref()
				.and_then(|key| self.presentation_matrices.get(key))
				.copied()
				.unwrap_or(matrix);
			let instance = &entry.instance;
			if instance.animation_palette().is_some() {
				validate_animation_placement(&entry.name, matrix)?;
			}
			stats.total_entities += 1;
			if !entry.renderable {
				continue;
			}
			let outside = frustum.is_some_and(|frustum| {
				instance
					.animation_bounds()
					.or_else(|| instance.model().bounds())
					.and_then(|bounds| bounds.transformed(matrix).ok())
					.is_some_and(|bounds| !frustum.intersects(bounds))
			});
			if outside {
				stats.culled_entities += 1;
				continue;
			}
			stats.visible_entities += 1;
			let mut constants = ModelPushConstants::identity();
			constants.model = (matrix * instance.animation_mesh_matrix()).to_cols_array();
			if let Some(material) = &entry.material {
				constants.base_color_factor = material.base_color_factor;
			}
			objects
				.try_reserve(instance.model().parts().len())
				.map_err(|error| format!("Cannot reserve scene render objects: {error}"))?;
			for part in instance.model().parts() {
				stats.draw_calls += 1;
				stats.triangles += part.mesh().index_count() as usize / 3;
				let mut object = RenderObject::new(part.mesh(), part.material(), constants);
				if let Some(palette) = instance.animation_palette() {
					object = object.with_skin_palette(palette);
				}
				objects.push(object);
			}
		}

		Ok((objects, stats))
	}

	pub(crate) fn subtree_render_object_count(&self, id: EntityId) -> Result<usize, String> {
		let root = *self
			.id_indices
			.get(&id)
			.ok_or("Duplicate source entity is missing")?;
		let mut children = vec![Vec::new(); self.entries.len()];
		for (index, entry) in self.entries.iter().enumerate() {
			if let Some(parent) = entry.parent.as_ref() {
				let parent = self
					.key_ids
					.get(parent)
					.and_then(|id| self.id_indices.get(id))
					.copied()
					.ok_or("Duplicate source parent is missing")?;
				children[parent].push(index);
			}
		}
		let mut pending = vec![root];
		let mut visited = vec![false; self.entries.len()];
		let mut count = 0usize;
		while let Some(index) = pending.pop() {
			if visited[index] {
				return Err("Duplicate source hierarchy contains a cycle".into());
			}
			visited[index] = true;
			count = count
				.checked_add(self.entries[index].instance.model().parts().len())
				.ok_or("Render-object count overflow")?;
			pending.extend_from_slice(&children[index]);
		}
		Ok(count)
	}

	pub fn renderer_sky(&self) -> Option<renderer::vulkan::SkyGradient> {
		let key = self.environment.active_sky.as_ref()?;
		let id = self.key_ids.get(key)?;
		let entry = &self.entries[*self.id_indices.get(id)?];
		let sky = entry
			.sky
			.as_ref()
			.filter(|sky| sky.enabled && entry.parent.is_none())?;
		Some(renderer::vulkan::SkyGradient {
			zenith: sky.settings.zenith,
			horizon: sky.settings.horizon,
		})
	}

	pub fn renderer_lights(&self) -> Vec<renderer::vulkan::Light> {
		self.lights
			.iter()
			.map(|light| match light.kind {
				LightKind::Directional => renderer::vulkan::Light::Directional {
					direction: light.direction,
					color: light.color,
					intensity: light.intensity,
				},
				LightKind::Point => renderer::vulkan::Light::Point {
					position: light.position,
					color: light.color,
					intensity: light.intensity,
					range: light.range,
				},
			})
			.collect()
	}
}
