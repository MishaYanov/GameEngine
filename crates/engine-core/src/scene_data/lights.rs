//! Compatibility mapping for authored directional and point light objects.
use glam::{EulerRot, Quat, Vec3};
use std::collections::HashMap;

use super::{EntityData, LightData, LightKind, MAX_SCENE_ENTITIES, SceneData, TransformData};
use crate::components::{ComponentId, LightComponent, LightSettings, NO_MESH_PATH};

impl LightData {
	pub(crate) fn object_transform(&self) -> Result<TransformData, String> {
		if !self
			.position
			.iter()
			.chain(&self.direction)
			.all(|value| value.is_finite())
		{
			return Err("Light position and direction must be finite".into());
		}
		let rotation = if self.kind == LightKind::Directional {
			let direction = Vec3::from_array(self.direction);
			if direction.length_squared() < 1.0e-12 {
				return Err("Directional lights require a nonzero direction".into());
			}
			let direction = direction
				.try_normalize()
				.ok_or("Directional lights require a finite direction length")?;
			let (x, y, z) = Quat::from_rotation_arc(Vec3::NEG_Z, direction).to_euler(EulerRot::XYZ);
			[x, y, z]
		} else {
			[0.0; 3]
		};
		Ok(TransformData {
			position: self.position,
			rotation,
			..Default::default()
		})
	}

	pub(crate) fn to_object(&self) -> Result<EntityData, String> {
		let settings = LightSettings::from(self);
		settings.validate()?;
		let mut entity = EntityData::new(self.key.clone(), &self.name, NO_MESH_PATH)
			.with_renderable(false)
			.with_transform(self.object_transform()?);
		entity.light = Some(Box::new(LightComponent {
			id: ComponentId::new("light")?,
			enabled: true,
			settings,
		}));
		Ok(entity)
	}
}

impl SceneData {
	pub(crate) fn light_count(&self) -> usize {
		self.lights.len()
			+ self
				.entities
				.iter()
				.filter(|entity| entity.light.is_some())
				.count()
	}

	/// A legacy light maps to an object with its original stable key.
	pub fn migrate_legacy_lights(&mut self) -> Result<(), String> {
		if self.lights.is_empty() {
			return Ok(());
		}
		if self.entities.len() + self.lights.len() > MAX_SCENE_ENTITIES {
			return Ok(());
		}
		let objects = self
			.lights
			.iter()
			.map(LightData::to_object)
			.collect::<Result<Vec<_>, _>>()?;
		self.entities.extend(objects);
		self.lights.clear();
		Ok(())
	}

	/// The editor's compatibility list and Prop previews use the current object poses.
	pub fn projected_lights(&self) -> Result<Vec<LightData>, String> {
		if !self.entities.iter().any(|entity| entity.light.is_some()) {
			return Ok(self.lights.clone());
		}
		let world = self.world_matrices()?;
		let nodes: Option<HashMap<_, _>> =
			self.entities.iter().any(|entity| !entity.active).then(|| {
				self.entities
					.iter()
					.map(|entity| (&entity.key, entity))
					.collect()
			});
		let mut lights = self.lights.clone();
		for (entity, world) in self.entities.iter().zip(world) {
			let Some(light) = entity.light.as_ref().filter(|light| light.enabled) else {
				continue;
			};
			if let Some(nodes) = &nodes {
				let mut ancestry = std::iter::successors(Some(entity), |entity| {
					entity
						.parent
						.as_ref()
						.and_then(|key| nodes.get(key).copied())
				});
				if ancestry.any(|entity| !entity.active) {
					continue;
				}
			}
			lights.push(light.settings.project(&entity.key, &entity.name, world));
		}
		Ok(lights)
	}
}
