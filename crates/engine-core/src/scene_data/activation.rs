use std::collections::HashMap;

use super::SceneData;

impl SceneData {
	/// Projects authored activation into the isolated Play document after hierarchy validation.
	pub(crate) fn project_runtime_activation(&mut self) {
		if self.entities.iter().all(|entity| entity.active) {
			return;
		}

		let indices: HashMap<_, _> = self
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (&entity.key, index))
			.collect();
		let mut states = vec![None; self.entities.len()];
		let mut lineage = Vec::new();
		for index in 0..self.entities.len() {
			let mut current = Some(index);
			while let Some(index) = current.filter(|index| states[*index].is_none()) {
				lineage.push(index);
				current = self.entities[index].parent.as_ref().map(|key| indices[key]);
			}
			let mut active = current.and_then(|index| states[index]).unwrap_or(true);
			for index in lineage.drain(..).rev() {
				active &= self.entities[index].active;
				states[index] = Some(active);
			}
		}

		if self
			.default_game_camera
			.as_ref()
			.is_some_and(|key| states[indices[key]] == Some(false))
		{
			self.default_game_camera = None;
			self.camera = Default::default();
		}
		if self
			.environment
			.active_sky
			.as_ref()
			.is_some_and(|key| states[indices[key]] == Some(false))
		{
			self.environment.active_sky = None;
		}
		for (entity, active) in self.entities.iter_mut().zip(states) {
			if active == Some(false) {
				entity.active = false;
				entity.renderable = false;
				entity.collision_enabled = false;
				entity.native_behaviors.clear();
				entity.scripts.clear();
				entity.character_controller = None;
				entity.player_controller = None;
				entity.animation = None;
				entity.animator = None;
				entity.interactable = None;
				entity.camera = None;
				entity.audio_source = None;
				entity.sky = None;
				entity.particle_emitter = None;
			}
		}
	}
}
