use renderer::{
	EditorShadingMode, WindowRenderer,
	vulkan::{CameraUniform, ModelPushConstants, RenderObject},
};
use std::collections::HashSet;

use super::{EntityId, MAX_SCENE_RENDER_OBJECTS, Scene, validate_animation_placement};
use crate::{
	scene_data::{LightData, LightKind, SceneEnvironment},
	visibility::Frustum,
};

/// Camera and directional-light frusta for a single scene submission.
#[derive(Default)]
pub struct RenderVisibility {
	camera: Option<Frustum>,
	shadow: Option<Frustum>,
}

impl RenderVisibility {
	pub fn from_camera(
		renderer: &WindowRenderer,
		camera: &CameraUniform,
		shading: Option<EditorShadingMode>,
	) -> Result<Self, String> {
		let camera_frustum =
			Frustum::from_view_projection(glam::Mat4::from_cols_array(&camera.view_projection))?;
		let shadow = renderer
			.directional_shadow_view_projection(camera, shading)
			.map(Frustum::from_view_projection)
			.transpose()?;
		Ok(Self {
			camera: Some(camera_frustum),
			shadow,
		})
	}
}

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
		self.try_render_filtered(frustum, None, &HashSet::new(), &HashSet::new())
	}

	/// Build an editor submission with vertex markers on the selected mesh objects.
	pub fn try_render_editor_visible(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
		selected: &[engine_core::EntityKey],
	) -> Result<(Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats), String> {
		let selected = selected.iter().collect();
		self.try_render_filtered(frustum, None, &HashSet::new(), &selected)
	}

	pub fn try_render_world_detail<'a>(
		&self,
		frustum: Option<&crate::visibility::Frustum>,
		lods: impl IntoIterator<Item=&'a engine_core::project::WorldLod>,
		eye: [f32; 3],
	) -> Result<(Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats), String> {
		let hidden = self.hidden_world_lods(lods, eye);
		self.try_render_filtered(frustum, None, &hidden, &HashSet::new())
	}

	/// Submit camera-visible geometry and casters that intersect the light's view.
	pub fn try_render_view<'a>(
		&self,
		visibility: &RenderVisibility,
		selected: &[engine_core::EntityKey],
		lods: impl IntoIterator<Item=&'a engine_core::project::WorldLod>,
		eye: [f32; 3],
	) -> Result<(Vec<RenderObject<'_>>, crate::visibility::SceneRenderStats), String> {
		let hidden = self.hidden_world_lods(lods, eye);
		let selected = selected.iter().collect();
		self.try_render_filtered(
			visibility.camera.as_ref(),
			visibility.shadow.as_ref(),
			&hidden,
			&selected,
		)
	}

	fn hidden_world_lods<'a>(
		&self,
		lods: impl IntoIterator<Item=&'a engine_core::project::WorldLod>,
		eye: [f32; 3],
	) -> HashSet<&'a engine_core::EntityKey> {
		let mut hidden = HashSet::new();
		for lod in lods {
			let has_detail = self
				.entity_by_key(&lod.detail)
				.is_some_and(|id| self.is_renderable(id));
			let has_proxy = self
				.entity_by_key(&lod.proxy)
				.is_some_and(|id| self.is_renderable(id));
			if has_detail && has_proxy {
				hidden.insert(if lod.prefer_detail(eye) {
					&lod.proxy
				} else {
					&lod.detail
				});
			}
		}
		hidden
	}

	fn try_render_filtered(
		&self,
		frustum: Option<&Frustum>,
		shadow: Option<&Frustum>,
		hidden: &HashSet<&engine_core::EntityKey>,
		selected: &HashSet<&engine_core::EntityKey>,
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
			if !entry.renderable || entry.key.as_ref().is_some_and(|key| hidden.contains(key)) {
				continue;
			}
			let bounds = (frustum.is_some() || shadow.is_some())
				.then(|| {
					instance
						.animation_bounds()
						.or_else(|| instance.model().bounds())
				})
				.flatten()
				.and_then(|bounds| bounds.transformed(matrix).ok());
			let visible = frustum
				.zip(bounds)
				.is_none_or(|(view, bounds)| view.intersects(bounds));
			let shadow_caster = shadow.map_or(visible, |view| {
				bounds.is_none_or(|bounds| view.intersects(bounds))
			});
			if visible {
				stats.visible_entities += 1;
			} else {
				stats.culled_entities += 1;
				if !shadow_caster {
					continue;
				}
			}
			let mut constants = ModelPushConstants::identity();
			constants.model = (matrix * instance.animation_mesh_matrix()).to_cols_array();
			if let Some(material) = &entry.material {
				constants.base_color_factor = material.base_color_factor;
			}
			objects
				.try_reserve(instance.model().parts().len())
				.map_err(|error| format!("Cannot reserve scene render objects: {error}"))?;
			let show_vertices =
				visible && entry.key.as_ref().is_some_and(|key| selected.contains(key));
			for part in instance.model().parts() {
				if visible {
					stats.draw_calls += 1;
					stats.triangles += part.mesh().index_count() as usize / 3;
				}
				let mut object = RenderObject::new(part.mesh(), part.material(), constants)
					.with_pass_visibility(visible, shadow_caster);
				if let Some(palette) = instance.animation_palette() {
					object = object.with_skin_palette(palette);
				}
				if show_vertices {
					object = object.with_vertex_markers();
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

	pub fn sky_settings(&self) -> Option<&engine_core::components::SkySettings> {
		let key = self.environment.active_sky.as_ref()?;
		let id = self.key_ids.get(key)?;
		let entry = &self.entries[*self.id_indices.get(id)?];
		let sky = entry
			.sky
			.as_ref()
			.filter(|sky| sky.enabled && entry.parent.is_none())?;
		Some(&sky.settings)
	}

	pub fn configure_sky(
		renderer: &mut renderer::WindowRenderer,
		assets: Option<&mut crate::assets::AssetManager>,
		settings: Option<&engine_core::components::SkySettings>,
		environment: &SceneEnvironment,
	) -> Result<(), String> {
		let gradient = settings.map(|sky| renderer::vulkan::SkyGradient {
			zenith: sky.zenith,
			horizon: sky.horizon,
			yaw_degrees: sky.yaw_degrees,
			exposure_ev: environment.sky_exposure_ev,
		});
		let image = settings.and_then(|sky| sky.background.image());
		let texture = assets.and_then(|assets| assets.sky_texture(renderer, image));
		renderer.set_sky_panorama(texture)?;
		renderer.set_sky(gradient);
		renderer.set_ambient(environment.ambient_linear)?;
		renderer.set_fog(environment.fog.map(|fog| renderer::vulkan::Fog {
			color_linear: fog.color_linear,
			density: fog.density,
			base_height: fog.base_height,
			height_falloff: fog.height_falloff,
			start_distance: fog.start_distance,
		}))
	}

	pub fn environment(&self) -> &SceneEnvironment {
		&self.environment
	}

	pub fn renderer_lights(&self) -> Vec<renderer::vulkan::Light> {
		let mut output = Self::project_renderer_lights(&self.lights);
		if !self.has_light_objects() {
			return output;
		}
		if let Ok(matrices) = self.cached_world_matrices() {
			output.extend(self.light_objects.iter().filter_map(|id| {
				let index = *self.id_indices.get(id)?;
				let entry = &self.entries[index];
				let light = entry.light.as_ref().filter(|light| light.enabled)?;
				let mut ancestor = Some(entry);
				while let Some(current) = ancestor {
					if !current.active {
						return None;
					}
					ancestor = current
						.parent
						.as_ref()
						.and_then(|key| self.key_ids.get(key))
						.and_then(|id| self.id_indices.get(id))
						.and_then(|&index| self.entries.get(index));
				}
				let settings = light.settings;
				let matrix = matrices[index];
				Some(match settings.kind {
					LightKind::Directional => renderer::vulkan::Light::Directional {
						direction: engine_core::components::LightSettings::world_direction(matrix),
						color: settings.color,
						intensity: settings.intensity,
					},
					LightKind::Point => renderer::vulkan::Light::Point {
						position: matrix.w_axis.truncate().to_array(),
						color: settings.color,
						intensity: settings.intensity,
						range: settings.range,
					},
				})
			}));
		}
		output
	}

	pub fn project_renderer_lights(lights: &[LightData]) -> Vec<renderer::vulkan::Light> {
		lights.iter().map(renderer_light).collect()
	}
}

fn renderer_light(light: &LightData) -> renderer::vulkan::Light {
	match light.kind {
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
	}
}
