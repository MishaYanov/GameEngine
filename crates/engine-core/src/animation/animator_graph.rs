//! Versioned Animator definitions. Ordered edges define runtime priority.
use super::{AnimationClipRef, AnimationSourceRef, VisualTargetId};
use crate::{EntityKey, asset_catalog::AssetId, input::valid_game_action_id};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const ANIMATOR_GRAPH_SUFFIX: &str = ".animator.json";
pub const MAX_ANIMATOR_GRAPH_BYTES: usize = 256 * 1024;
const MAX_STATES: usize = 64;
const MAX_TRANSITIONS: usize = 256;
const MAX_PARAMETERS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorSettings {
	pub asset_id: AssetId,
	pub binding: AnimatorBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimatorBinding {
	Imported {
		source: AnimationSourceRef,
	},
	Visual {
		#[serde(default, skip_serializing_if = "Vec::is_empty")]
		targets: Vec<AnimatorVisualTarget>,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorVisualTarget {
	pub channel: VisualTargetId,
	pub entity_key: EntityKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorGraph {
	pub schema_version: u32,
	pub entry: String,
	pub states: Vec<AnimatorState>,
	#[serde(default)]
	pub parameters: Vec<AnimatorParameter>,
	#[serde(default)]
	pub transitions: Vec<AnimatorTransition>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub layout: BTreeMap<String, AnimatorNodePosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorNodePosition {
	pub x: i32,
	pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorState {
	pub id: String,
	pub name: String,
	pub clip: AnimationClipRef,
	#[serde(default)]
	pub looping: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorParameter {
	pub id: String,
	pub name: String,
	pub source: AnimatorParameterSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimatorParameterSource {
	Boolean {
		#[serde(default)]
		default: bool,
	},
	Grounded,
	Moving {
		minimum_mm_per_second: u16,
	},
	Jumped,
	ActionAccepted {
		action: String,
	},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorTransition {
	pub id: String,
	pub from: String,
	pub to: String,
	pub condition: AnimatorCondition,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub exit_after_ticks: Option<u32>,
	#[serde(default)]
	pub blend_ticks: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimatorCondition {
	Always,
	Bool { parameter: String, value: bool },
	Trigger { parameter: String },
}

impl AnimatorParameterSource {
	pub fn trigger(&self) -> bool {
		matches!(self, Self::Jumped | Self::ActionAccepted { .. })
	}
}

impl AnimatorSettings {
	pub fn validate(&self) -> Result<(), String> {
		if let AnimatorBinding::Imported { source } = &self.binding {
			AnimationClipRef::Imported {
				source: source.clone(),
				clip_index: 0,
			}
			.validate()?;
		}
		if let AnimatorBinding::Visual { targets } = &self.binding {
			let mut channels = std::collections::BTreeSet::new();
			if targets.len() > 128
				|| targets
					.iter()
					.any(|target| !channels.insert(&target.channel))
			{
				return Err("Animator requires at most 128 uniquely bound visual channels".into());
			}
			for target in targets {
				target.entity_key.validate()?;
			}
		}
		Ok(())
	}
}

impl AnimatorGraph {
	pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
		if bytes.len() > MAX_ANIMATOR_GRAPH_BYTES {
			return Err("Animator asset exceeds 256 KiB".into());
		}
		let graph: Self =
			serde_json::from_slice(bytes).map_err(|error| format!("Animator asset: {error}"))?;
		graph.validate()?;
		Ok(graph)
	}

	pub fn to_json(&self) -> Result<Vec<u8>, String> {
		self.validate()?;
		let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_ANIMATOR_GRAPH_BYTES {
			return Err("Formatted Animator asset exceeds 256 KiB".into());
		}
		Ok(bytes)
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != 1
			|| !(1..=MAX_STATES).contains(&self.states.len())
			|| self.parameters.len() > MAX_PARAMETERS
			|| self.transitions.len() > MAX_TRANSITIONS
		{
			return Err("Animator schema 1 requires 1–64 states and at most 64 parameters and 256 transitions".into());
		}
		let mut states = HashSet::new();
		for state in &self.states {
			if !valid_game_action_id(&state.id) || !states.insert(state.id.as_str()) {
				return Err(format!(
					"Animator state {:?} has an invalid or duplicate ID",
					state.id
				));
			}
			validate_label("state", &state.id, &state.name)?;
			state
				.clip
				.validate()
				.map_err(|error| format!("Animator state {:?}: {error}", state.id))?;
		}
		if !states.contains(self.entry.as_str()) {
			return Err(format!(
				"Animator entry {:?} must name an existing state",
				self.entry
			));
		}
		let mut parameters = HashMap::new();
		for parameter in &self.parameters {
			if !valid_game_action_id(&parameter.id)
				|| parameters
					.insert(parameter.id.as_str(), parameter.source.trigger())
					.is_some()
			{
				return Err(format!(
					"Animator parameter {:?} has an invalid or duplicate ID",
					parameter.id
				));
			}
			validate_label("parameter", &parameter.id, &parameter.name)?;
			match &parameter.source {
				AnimatorParameterSource::ActionAccepted { action }
					if !valid_game_action_id(action) =>
				{
					return Err(format!(
						"Animator parameter {:?} requires a valid accepted action ID",
						parameter.id
					));
				}
				AnimatorParameterSource::Moving {
					minimum_mm_per_second,
				} if !(1..=10_000).contains(minimum_mm_per_second) => {
					return Err(format!(
						"Animator parameter {:?} requires a speed threshold between 1 and 10,000 mm/s",
						parameter.id
					));
				}
				_ => {}
			}
		}
		let mut edges = HashSet::new();
		let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
		for edge in &self.transitions {
			if !valid_game_action_id(&edge.id) || !edges.insert(edge.id.as_str()) {
				return Err(format!(
					"Animator transition {:?} has an invalid or duplicate ID",
					edge.id
				));
			}
			if !states.contains(edge.from.as_str()) || !states.contains(edge.to.as_str()) {
				return Err(format!(
					"Animator transition {:?} requires existing source and destination states",
					edge.id
				));
			}
			if edge.exit_after_ticks.is_some_and(|ticks| ticks > 1_000_000)
				|| edge.blend_ticks > 3600
			{
				return Err(format!(
					"Animator transition {:?} exceeds its exit or blend duration limit",
					edge.id
				));
			}
			let compatible = match &edge.condition {
				AnimatorCondition::Always => true,
				AnimatorCondition::Bool { parameter, .. } => {
					parameters.get(parameter.as_str()) == Some(&false)
				}
				AnimatorCondition::Trigger { parameter } => {
					parameters.get(parameter.as_str()) == Some(&true)
				}
			};
			if !compatible {
				return Err(format!(
					"Animator transition {:?} references a missing or wrongly typed parameter",
					edge.id
				));
			}
			adjacency.entry(&edge.from).or_default().push(&edge.to);
		}
		let mut reachable = HashSet::new();
		let mut queue = vec![self.entry.as_str()];
		while let Some(id) = queue.pop() {
			if reachable.insert(id)
				&& let Some(targets) = adjacency.get(id)
			{
				queue.extend(targets.iter().copied());
			}
		}
		if let Some(state) = self
			.states
			.iter()
			.find(|state| !reachable.contains(state.id.as_str()))
		{
			return Err(format!(
				"Animator state {:?} is unreachable from the entry",
				state.id
			));
		}
		if self.layout.iter().any(|(id, position)| {
			!states.contains(id.as_str())
				|| position.x.abs_diff(0) > 100_000
				|| position.y.abs_diff(0) > 100_000
		}) {
			return Err("Animator layout has a missing state or exceeds the canvas bounds".into());
		}
		Ok(())
	}

	pub fn validate_binding(&self, binding: &AnimatorBinding) -> Result<(), String> {
		self.validate()?;
		for state in &self.states {
			let matches = match (binding, &state.clip) {
				(
					AnimatorBinding::Imported { source: rig },
					AnimationClipRef::Imported { source, .. },
				) => rig == source,
				(AnimatorBinding::Visual { .. }, AnimationClipRef::Authored { .. }) => true,
				_ => false,
			};
			if !matches {
				return Err(format!(
					"Animator state {:?} is incompatible with the actor's rig or visual binding",
					state.id
				));
			}
		}
		Ok(())
	}

	/// Node coordinates and display labels do not affect runtime selection.
	pub fn runtime_digest(&self) -> Result<String, String> {
		self.validate()?;
		let mut runtime = self.clone();
		runtime.layout.clear();
		for state in &mut runtime.states {
			state.name.clear();
		}
		for parameter in &mut runtime.parameters {
			parameter.name.clear();
		}
		let bytes = serde_json::to_vec(&runtime).map_err(|error| error.to_string())?;
		Ok(format!("{:x}", Sha256::digest(bytes)))
	}
}

fn validate_label(kind: &str, id: &str, name: &str) -> Result<(), String> {
	if name.trim() != name
		|| name.is_empty()
		|| name.len() > 96
		|| name.chars().any(char::is_control)
	{
		return Err(format!(
			"Animator {kind} {id:?} requires a display name of 1–96 bytes"
		));
	}
	Ok(())
}
