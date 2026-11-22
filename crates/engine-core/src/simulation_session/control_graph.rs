use super::{InputFrame, SimulationSession};
use crate::{
	SceneData,
	camera_input::CameraIntent,
	input::{
		ActionBinding, CompiledControlGraph, ControlActionSource, ControlButton,
		ControlGraphOutput, ControlIntent, ControlLook, InputActionMap, ResolvedInputActions,
	},
	project::ProjectData,
	scene_data::EntityData,
};
use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
};

pub type ControlGraphResolver = Arc<
	dyn Fn(
			&ProjectData,
			&SceneData,
			Option<&InputActionMap>,
		) -> Result<Option<CompiledControlGraph>, String>
		+ Send
		+ Sync,
>;

#[derive(Clone, Default)]
pub(super) struct ControlSample {
	pub releases: HashSet<String>,
	vectors: BTreeMap<String, [f32; 2]>,
	look_rates: BTreeMap<String, [f32; 2]>,
	needs_step: bool,
}

struct ActionStep<'a> {
	input: &'a InputFrame,
	typed: bool,
	pointer_action: Option<&'a str>,
	pointer: [f32; 2],
}

impl ControlSample {
	pub fn clear(&mut self, active: bool) {
		*self = Self {
			needs_step: active,
			..Self::default()
		};
	}

	pub fn update_axes(&mut self, sample: ResolvedInputActions) {
		self.needs_step |= self.vectors != sample.vectors || self.look_rates != sample.look_rates;
		self.vectors = sample.vectors;
		self.look_rates = sample.look_rates;
	}

	pub fn finish_step(&mut self) {
		self.releases.clear();
		self.needs_step = false;
	}

	pub fn needs_tick(&self) -> bool {
		self.needs_step
			|| !self.releases.is_empty()
			|| self
				.vectors
				.values()
				.chain(self.look_rates.values())
				.any(|&value| value != [0.0; 2])
	}
}

impl ControlActionSource for ActionStep<'_> {
	fn button(&self, action: &str) -> ControlButton {
		ControlButton {
			held: self.input.action_held(action),
			pressed: self.input.action_pressed(action),
			released: self.input.control_sample.releases.contains(action),
		}
	}

	fn vector(&self, action: &str) -> [f32; 2] {
		if self.typed {
			self.input
				.control_sample
				.vectors
				.get(action)
				.copied()
				.unwrap_or_default()
		} else if action == "move" {
			self.input.controller_movement
		} else {
			[0.0; 2]
		}
	}

	fn look(&self, action: &str) -> ControlLook {
		let delta = if self.pointer_action == Some(action) {
			self.pointer
		} else {
			[0.0; 2]
		};
		let rate = if self.typed {
			self.input
				.control_sample
				.look_rates
				.get(action)
				.copied()
				.unwrap_or_default()
		} else if action == "look" {
			self.input.controller_look
		} else {
			[0.0; 2]
		};
		ControlLook { delta, rate }
	}
}

impl SimulationSession {
	pub fn set_control_graph_resolver(
		&mut self,
		resolver: &ControlGraphResolver,
	) -> Result<(), String> {
		let candidate = resolver(
			&self.project,
			self.world.data(),
			self.input_action_map.as_ref(),
		)?;
		if candidate.is_none()
			&& let Some(actor) = self.world.data().local_player()
			&& actor
				.player_controller
				.as_ref()
				.is_some_and(|player| player.settings.control_graph.is_some())
		{
			return Err(format!(
				"Player Controller on {:?} requires a prepared Controls Graph",
				actor.key.as_str()
			));
		}
		self.control_graph = candidate;
		self.control_graph_ready = true;
		self.inputs
			.control_sample
			.clear(self.control_graph.is_some());
		Ok(())
	}

	pub fn require_control_graph_ready(&self) -> Result<(), String> {
		if !self.control_graph_ready
			&& let Some(actor) = first_configured_actor(self.world.data())
		{
			return Err(format!(
				"Player Controller on {:?} requires a prepared Controls Graph",
				actor.key.as_str()
			));
		}
		Ok(())
	}

	pub(super) fn evaluate_control_graph(
		&self,
		camera: CameraIntent,
	) -> Option<ControlGraphOutput> {
		let graph = self.control_graph.as_ref()?;
		let pointer_action = match self.input_action_map.as_ref() {
			Some(map) => pointer_action(map),
			None => Some("look"),
		};
		Some(graph.evaluate(&ActionStep {
			input: &self.inputs,
			typed: self.input_action_map.is_some(),
			pointer_action,
			pointer: [camera.yaw_delta as f32, camera.pitch_delta as f32],
		}))
	}
}

impl InputFrame {
	pub(super) fn with_control_output(&self, output: ControlIntent) -> Self {
		let mut routed = self.clone();
		routed.controller_movement = output.movement;
		routed.controller_look = output.look.rate;
		for (action, button) in [("jump", output.jump), ("attack", output.attack)] {
			routed.game_actions.ids.remove(action);
			routed.action_presses.remove(action);
			if button.held {
				routed.game_actions.ids.insert(action.into());
			}
			if button.pressed {
				routed.action_presses.insert(action.into());
			}
		}
		routed
	}
}

pub(super) fn pointer_action(map: &InputActionMap) -> Option<&str> {
	map.actions
		.iter()
		.find(|action| action.bindings.contains(&ActionBinding::PointerDelta))
		.map(|action| action.id.as_str())
}

pub(super) fn first_configured_actor(scene: &SceneData) -> Option<&EntityData> {
	scene.entities.iter().find(|actor| {
		actor
			.player_controller
			.as_ref()
			.is_some_and(|player| player.enabled && player.settings.control_graph.is_some())
	})
}
