use super::{RuntimeEntityHandle, RuntimeLifecycle};
use crate::{
	components::{ComponentId, NativeBehaviorSpec},
	world::RuntimeWorld,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBehaviorStateDto {
	pub component_id: ComponentId,
	pub kind: String,
	pub activated: bool,
	pub elapsed_ticks: u32,
	pub machine_state: Option<String>,
	pub entered_step: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEntityDto {
	pub handle: RuntimeEntityHandle,
	pub name: String,
	pub spawned: bool,
	pub instance_id: Option<String>,
	pub behavior_states: Vec<RuntimeBehaviorStateDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlGraphStepDto {
	pub actor: RuntimeEntityHandle,
	pub asset_id: crate::asset_catalog::AssetId,
	pub tick: u64,
	pub active_node_ids: Vec<String>,
	pub changed_outputs: Vec<String>,
	pub output: crate::input::ControlGraphOutput,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInspectionDto {
	pub play_session_id: String,
	pub run_id: String,
	pub scene_id: crate::scene_catalog::SceneId,
	pub tick: u64,
	pub topology_revision: u64,
	pub total_entities: usize,
	pub entities: Vec<RuntimeEntityDto>,
	pub timer_count: usize,
	pub pending_spawns: usize,
	pub pending_events: usize,
	pub event_sequence: u64,
	pub machine_deadlines: usize,
	pub stale_events_discarded: u64,
	pub character_motion: Vec<crate::character_controller::CommittedMotion>,
	pub animations: Vec<crate::animation::AnimationInspection>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub animators: Vec<crate::animation::AnimatorInspection>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub control_graph: Option<ControlGraphStepDto>,
	pub last_error: Option<String>,
}

impl RuntimeLifecycle {
	pub fn entities(
		&self,
		world: &RuntimeWorld,
		offset: usize,
		limit: usize,
	) -> Vec<RuntimeEntityDto> {
		world
			.data()
			.entities
			.iter()
			.skip(offset)
			.take(limit.min(128))
			.map(|entity| {
				let instance_id = self.instance_for(&entity.key).map(str::to_owned);
				RuntimeEntityDto {
					handle: self.handle(&entity.key).expect("live world has handles"),
					name: entity.name.clone(),
					spawned: instance_id.is_some(),
					instance_id,
					behavior_states: entity
						.native_behaviors
						.iter()
						.map(|component| {
							let state = self
								.states
								.get(&(entity.key.clone(), component.id.clone()))
								.cloned()
								.unwrap_or_default();
							RuntimeBehaviorStateDto {
								component_id: component.id.clone(),
								kind: match &component.behavior {
									NativeBehaviorSpec::KeyDoor { .. } => "key_door",
									NativeBehaviorSpec::Collectible { .. } => "collectible",
									NativeBehaviorSpec::TimedSpawner { .. } => "timed_spawner",
									NativeBehaviorSpec::StateMachine { .. } => "state_machine",
								}
								.into(),
								activated: state.activated,
								elapsed_ticks: state.elapsed_ticks,
								machine_state: self
									.machine_states
									.get(&(entity.key.clone(), component.id.clone()))
									.filter(|state| state.initialized)
									.and_then(|state| {
										self.machines
											.get(&(entity.key.clone(), component.id.clone()))
											.map(|definition| {
												definition.states[state.state].id.clone()
											})
									}),
								entered_step: self
									.machine_states
									.get(&(entity.key.clone(), component.id.clone()))
									.filter(|state| state.initialized)
									.map(|state| state.entered_step),
							}
						})
						.collect(),
				}
			})
			.collect()
	}
	pub fn timer_count(&self) -> usize {
		self.timers.len()
	}
	pub fn request_count(&self) -> usize {
		self.requests.len()
	}
	/// Deterministic native-state evidence. UUID ownership is normalized to a
	/// same-world predicate while key/generation identity remains observable.
	pub fn replay_state(&self) -> serde_json::Value {
		let handle = |h: &RuntimeEntityHandle| serde_json::json!({"entity":h.key,"generation":h.generation,"same_world":h.world_id==self.world_id});
		let machines = self.machines.iter().map(|((key, id), definition)| {
			let state = self.machine_states.get(&(key.clone(), id.clone())).cloned().unwrap_or_default();
			let index = if state.initialized { state.state } else { definition.initial };
			serde_json::json!({"entity":key,"generation":self.generations.get(key),"component":id,"state":definition.states[index].id,"entered_step":state.entered_step,"initialized":state.initialized})
		}).collect::<Vec<_>>();
		serde_json::json!({
			"events":self.events.replay_state(&self.world_id),"stale_events_discarded":self.stale_events_discarded,
			"machines":machines,"topology_revision":self.topology_revision,"next_instance":self.next_instance,"next_request":self.next_request,"generations":self.generations.as_ref(),
			"legacy_states":self.states.iter().map(|((key,id),state)|serde_json::json!({"entity":key,"component":id,"state":state})).collect::<Vec<_>>(),
			"timers":self.timers.iter().map(|timer|serde_json::json!({"target":handle(&timer.target),"due_tick":timer.due})).collect::<Vec<_>>(),
			"requests":self.requests.values().map(|r|serde_json::json!({"id":r.id,"owner":handle(&r.owner),"component":r.component_id,"prefab_path":r.prefab_path,"bindings":r.bindings,"position":r.position,"lifetime_ticks":r.lifetime_ticks})).collect::<Vec<_>>(),
			"instances":self.instances.by_id.iter().map(|(id,i)|serde_json::json!({"id":id,"entities":i.keys,"owner":i.owner.as_ref().map(handle),"component":i.component_id})).collect::<Vec<_>>()
		})
	}
}
