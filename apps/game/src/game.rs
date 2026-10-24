//! Explicit legacy demo migration. Normal Player and Play execution use the
//! authored project through SceneEditor/core SimulationSession directly.
use runtime::{
	Camera,
	scene::Scene,
	scene_editor::{BehaviorSpace, BehaviorSpec, ProjectData},
};
#[cfg(test)]
use runtime::{
	EntityKey, InputState, Key,
	scene::EntityId,
	scene_editor::{SimulationSession, movement_direction},
};
#[cfg(test)]
use std::sync::Arc;

pub(crate) fn legacy_project(
	scene: &Scene,
	name: &str,
	startup_scene: &str,
) -> Result<ProjectData, String> {
	let data = scene.to_data(&Camera::default())?;
	let mut project = ProjectData::for_scene(name, startup_scene, &data)?;
	if let (Some(left), Some(right)) = (scene.binding("robot_a"), scene.binding("robot_b")) {
		if left == right {
			return Err("The legacy robot demo requires distinct role bindings".into());
		}
		let left = scene
			.key(left)
			.ok_or("Legacy left robot has no persistent key")?
			.clone();
		let right = scene
			.key(right)
			.ok_or("Legacy right robot has no persistent key")?
			.clone();
		for behavior in [
			BehaviorSpec::ControlledMove {
				entity: left.clone(),
				speed: 1.0,
				space: BehaviorSpace::World,
			},
			BehaviorSpec::Spin {
				entity: left,
				angular_velocity: [0.6, 0.9, 0.0],
			},
			BehaviorSpec::Spin {
				entity: right,
				angular_velocity: [-0.5, 0.75, 0.2],
			},
		] {
			project.add_behavior(behavior);
		}
	}
	project.validate_scene(&data)?;
	Ok(project)
}

#[cfg(test)]
pub(crate) struct RobotGame {
	left_robot: EntityId,
	right_robot: EntityId,
	controlled: Vec<EntityKey>,
	simulation: SimulationSession,
}

#[cfg(test)]
impl RobotGame {
	pub(crate) fn from_scene(scene: &Scene) -> Result<Self, String> {
		let left_robot = scene
			.binding("robot_a")
			.ok_or("The robot demo requires binding \"robot_a\"")?;
		let right_robot = scene
			.binding("robot_b")
			.ok_or("The robot demo requires binding \"robot_b\"")?;
		Self::from_project(
			scene,
			legacy_project(scene, "Legacy Robot Demo", "scenes/robots.json")?,
			left_robot,
			right_robot,
		)
	}

	fn from_project(
		scene: &Scene,
		project: ProjectData,
		left_robot: EntityId,
		right_robot: EntityId,
	) -> Result<Self, String> {
		let controlled = project
			.behaviors_for(&project.startup_scene_id)
			.filter_map(|behavior| match behavior {
				BehaviorSpec::ControlledMove { entity, .. } => Some(entity.clone()),
				_ => None,
			})
			.collect();
		let simulation =
			SimulationSession::new(project, Arc::new(scene.to_data(&Camera::default())?))?;
		Ok(Self {
			left_robot,
			right_robot,
			controlled,
			simulation,
		})
	}

	pub(crate) fn animation_paused(&self) -> bool {
		self.simulation.paused()
	}
	pub(crate) fn toggle_pause(&mut self) {
		self.simulation.set_paused(!self.simulation.paused());
	}

	pub(crate) fn update(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		delta_seconds: f32,
	) -> Result<(), String> {
		if input.just_pressed(Key::Space) {
			self.toggle_pause();
		}
		self.tick(scene, movement_direction(input), delta_seconds)
	}

	pub(crate) fn tick(
		&mut self,
		scene: &mut Scene,
		movement: [f32; 2],
		elapsed: f32,
	) -> Result<(), String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Simulation time must be finite and nonnegative".into());
		}
		if self.simulation.paused() {
			return Ok(());
		}
		for entity in &self.controlled {
			self.simulation
				.set_input(entity, [movement[0], movement[1], 0.0])?;
		}
		let outcome = self.simulation.tick(elapsed as f64)?;
		if !outcome.changes.is_empty()
			&& !scene.synchronize_world(self.simulation.world(), &outcome.changes)?
		{
			return Err("Legacy demo GPU topology no longer matches its configured world".into());
		}
		Ok(())
	}
}

#[cfg(test)]
#[path = "tests_game.rs"]
mod tests;
