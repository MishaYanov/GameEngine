use engine_core::{
	EntityKey,
	animation::{AnimationBank, AnimatorInspection},
	asset_catalog::AssetId,
	components::ComponentId,
	input::ControlGraphOutput,
	runtime_lifecycle::RuntimeInspectionDto,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(super) struct GraphReplay<'a> {
	schema_version: u32,
	animators: Vec<AnimatorReplay<'a>>,
	control: Option<ControlReplay<'a>>,
	visual_world_matrices: BTreeMap<&'a EntityKey, [f32; 16]>,
}

#[derive(Serialize)]
struct AnimatorReplay<'a> {
	actor: &'a EntityKey,
	component: &'a ComponentId,
	state: &'a str,
	state_ticks: u64,
	transition: Option<&'a str>,
	blend_age: Option<u16>,
	blend_ticks: Option<u16>,
}

#[derive(Serialize)]
struct ControlReplay<'a> {
	actor: &'a EntityKey,
	asset_id: &'a AssetId,
	tick: u64,
	active_node_ids: &'a [String],
	changed_outputs: &'a [String],
	output: &'a ControlGraphOutput,
}

impl<'a> GraphReplay<'a> {
	pub(super) fn capture(
		runtime: &'a RuntimeInspectionDto,
		animations: Option<&'a AnimationBank>,
	) -> Option<Self> {
		if runtime.animators.is_empty() && runtime.control_graph.is_none() {
			return None;
		}
		Some(Self {
			schema_version: 1,
			animators: runtime.animators.iter().map(AnimatorReplay::from).collect(),
			control: runtime.control_graph.as_ref().map(|route| ControlReplay {
				actor: &route.actor.key,
				asset_id: &route.asset_id,
				tick: route.tick,
				active_node_ids: &route.active_node_ids,
				changed_outputs: &route.changed_outputs,
				output: &route.output,
			}),
			visual_world_matrices: animations
				.into_iter()
				.flat_map(|bank| bank.visual_matrices().iter())
				.map(|(key, matrix)| (key, matrix.to_cols_array()))
				.collect(),
		})
	}
}

impl<'a> From<&'a AnimatorInspection> for AnimatorReplay<'a> {
	fn from(animator: &'a AnimatorInspection) -> Self {
		Self {
			actor: &animator.entity.key,
			component: &animator.component_id,
			state: &animator.state_id,
			state_ticks: animator.state_ticks,
			transition: animator.transition.as_deref(),
			blend_age: animator.blend_age,
			blend_ticks: animator.blend_ticks,
		}
	}
}
