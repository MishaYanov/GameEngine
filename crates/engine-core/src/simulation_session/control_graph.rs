use super::{InputFrame, SimulationSession};
use crate::{
	EntityKey, SceneData,
	asset_catalog::AssetId,
	camera_input::CameraIntent,
	input::{
		ActionBinding, CompiledControlGraph, ControlActionSource, ControlButton,
		ControlGraphOutput, ControlIntent, ControlLook, InputActionMap, ResolvedInputActions,
	},
	project::ProjectData,
	runtime_lifecycle::{ControlGraphStepDto, RuntimeEntityHandle, RuntimeLifecycle},
	scene_data::EntityData,
	world::RuntimeWorld,
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

pub(super) struct CommittedControlStep {
	actor: RuntimeEntityHandle,
	asset_id: AssetId,
	tick: u64,
	active: u128,
	changed_outputs: Vec<String>,
	output: ControlGraphOutput,
}

impl CommittedControlStep {
	pub fn capture(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		owner: &EntityKey,
		tick: u64,
		sample: (ControlGraphOutput, u128),
		previous: Option<&Self>,
	) -> Option<Self> {
		let actor = lifecycle.handle(owner)?;
		let asset_id = world.data().entities[world.entity_index(owner)?]
			.player_controller
			.as_ref()?
			.settings
			.control_graph
			.as_ref()?
			.clone();
		let (output, active) = sample;
		let prior = previous.filter(|step| step.actor == actor && step.asset_id == asset_id);
		let prior_intent = prior.map(|step| step.output.intent).unwrap_or_default();
		let mut changed_outputs = Vec::new();
		for (name, changed) in [
			("move", output.intent.movement != prior_intent.movement),
			("look", output.intent.look != prior_intent.look),
			("jump", output.intent.jump != prior_intent.jump),
			("attack", output.intent.attack != prior_intent.attack),
		] {
			if changed {
				changed_outputs.push(name.into());
			}
		}
		for (parameter, value) in &output.presentation {
			if prior.and_then(|step| step.output.presentation.get(parameter)) != Some(value) {
				changed_outputs.push(format!("animator:{parameter}"));
			}
		}
		Some(Self {
			actor,
			asset_id,
			tick,
			active,
			changed_outputs,
			output,
		})
	}
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
		self.control_graph_step = None;
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
	) -> Option<(ControlGraphOutput, u128)> {
		let graph = self.control_graph.as_ref()?;
		let pointer_action = match self.effective_input_map() {
			Some(map) => pointer_action(map),
			None => Some("look"),
		};
		Some(graph.evaluate_with_activity(&ActionStep {
			input: &self.inputs,
			typed: self.input_action_map.is_some(),
			pointer_action,
			pointer: [camera.yaw_delta as f32, camera.pitch_delta as f32],
		}))
	}

	pub(super) fn control_graph_inspection(&self) -> Option<ControlGraphStepDto> {
		let step = self.control_graph_step.as_ref()?;
		let graph = self.control_graph.as_ref()?;
		if self.lifecycle.handle(&step.actor.key).as_ref() != Some(&step.actor) {
			return None;
		}
		Some(ControlGraphStepDto {
			actor: step.actor.clone(),
			asset_id: step.asset_id.clone(),
			tick: step.tick,
			active_node_ids: graph
				.active_node_ids(step.active)
				.map(str::to_owned)
				.collect(),
			changed_outputs: step.changed_outputs.clone(),
			output: step.output.clone(),
		})
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
