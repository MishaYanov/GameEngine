use super::*;
use crate::animation::{AnimatorResolver, PreparedAnimatorBindings};

impl SimulationSession {
	pub fn set_animator_resolver(&mut self, resolver: AnimatorResolver) -> Result<(), String> {
		let bindings = resolver(&self.project, &self.scene_id, self.world.data())?;
		self.install_animators(&bindings)?;
		self.animator_resolver = Some(resolver);
		Ok(())
	}

	fn install_animators(&mut self, bindings: &PreparedAnimatorBindings) -> Result<(), String> {
		let handles = self
			.world
			.data()
			.entities
			.iter()
			.filter_map(|entity| {
				self.lifecycle
					.handle(&entity.key)
					.map(|handle| (entity.key.clone(), handle))
			})
			.collect();
		self.animations
			.install_animators(&self.world, bindings, &handles, self.project.fixed_hz)
	}

	pub fn require_animators_ready(&self) -> Result<(), String> {
		if self.world.data().entities.iter().any(|entity| {
			entity
				.animator
				.as_ref()
				.is_some_and(|animator| animator.enabled)
		}) && !self.animations.has_animators()
		{
			return Err("Prepare Animator graph assets before starting this scene".into());
		}
		Ok(())
	}
}
