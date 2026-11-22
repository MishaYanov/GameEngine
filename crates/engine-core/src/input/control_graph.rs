//! Typed, fixed-step routing over resolved semantic input.

use super::{
	InputActionKind, InputActionMap, InputProfile, ResolvedInputActions, valid_game_action_id,
};
use crate::animation::{AnimatorGraph, AnimatorParameterSource};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTROL_GRAPH_SUFFIX: &str = ".controls.json";
pub const MAX_CONTROL_GRAPH_BYTES: usize = 128 * 1024;
const MAX_NODES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlGraph {
	pub schema_version: u32,
	pub nodes: Vec<ControlNode>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub layout: BTreeMap<String, ControlNodePosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlNodePosition {
	pub x: i32,
	pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlNode {
	pub id: String,
	pub data: ControlNodeData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlNodeData {
	Action {
		action: String,
		value: InputActionKind,
	},
	Boolean {
		value: bool,
	},
	ButtonHeld {
		input: String,
	},
	Not {
		input: String,
	},
	And {
		left: String,
		right: String,
	},
	Or {
		left: String,
		right: String,
	},
	Gate {
		condition: String,
		input: String,
	},
	Move {
		input: String,
	},
	Look {
		input: String,
	},
	Jump {
		input: String,
	},
	Attack {
		input: String,
	},
	AnimatorBool {
		parameter: String,
		input: String,
	},
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlButton {
	pub held: bool,
	pub pressed: bool,
	pub released: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlLook {
	pub delta: [f32; 2],
	pub rate: [f32; 2],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlIntent {
	pub movement: [f32; 2],
	pub look: ControlLook,
	pub jump: ControlButton,
	pub attack: ControlButton,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ControlGraphOutput {
	pub intent: ControlIntent,
	pub presentation: BTreeMap<String, bool>,
}

/// An immutable semantic action frame supplied by a fixed step or a dry run.
pub trait ControlActionSource {
	fn button(&self, action: &str) -> ControlButton;
	fn vector(&self, action: &str) -> [f32; 2];
	fn look(&self, action: &str) -> ControlLook;
}

pub struct ControlGraphFrame<'a> {
	pub actions: &'a ResolvedInputActions,
	pub pressed: &'a BTreeSet<String>,
	pub released: &'a BTreeSet<String>,
}

impl ControlActionSource for ControlGraphFrame<'_> {
	fn button(&self, action: &str) -> ControlButton {
		ControlButton {
			held: self.actions.buttons.contains(action),
			pressed: self.pressed.contains(action),
			released: self.released.contains(action),
		}
	}

	fn vector(&self, action: &str) -> [f32; 2] {
		self.actions
			.vectors
			.get(action)
			.copied()
			.unwrap_or_default()
	}

	fn look(&self, action: &str) -> ControlLook {
		ControlLook {
			delta: self.actions.deltas.get(action).copied().unwrap_or_default(),
			rate: self
				.actions
				.look_rates
				.get(action)
				.copied()
				.unwrap_or_default(),
		}
	}
}

#[derive(Debug, Clone)]
pub struct CompiledControlGraph {
	steps: Vec<CompiledNode>,
}

#[derive(Debug, Clone)]
struct CompiledNode {
	id: String,
	operation: CompiledOperation,
}

#[derive(Debug, Clone)]
enum CompiledOperation {
	Action(String, InputActionKind),
	Constant(bool),
	ButtonHeld(usize),
	Not(usize),
	And(usize, usize),
	Or(usize, usize),
	Gate(usize, usize),
	Output(usize, Output),
}

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
enum Output {
	Move,
	Look,
	Jump,
	Attack,
	AnimatorBool(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortType {
	Boolean,
	Button,
	Vector,
	Look,
}

#[derive(Debug, Clone, Copy, Default)]
enum Value {
	#[default]
	None,
	Boolean(bool),
	Button(ControlButton),
	Vector([f32; 2]),
	Look(ControlLook),
}

impl Value {
	fn boolean(self) -> bool {
		matches!(self, Self::Boolean(true))
	}

	fn active(self) -> bool {
		match self {
			Self::None => false,
			Self::Boolean(value) => value,
			Self::Button(value) => value != ControlButton::default(),
			Self::Vector(value) => value != [0.0; 2],
			Self::Look(value) => value != ControlLook::default(),
		}
	}
}

impl CompiledControlGraph {
	pub fn order(&self) -> impl Iterator<Item = &str> {
		self.steps.iter().map(|step| step.id.as_str())
	}

	pub fn evaluate(&self, source: &impl ControlActionSource) -> ControlGraphOutput {
		self.evaluate_inner::<false>(source).0
	}

	pub fn evaluate_with_activity(
		&self,
		source: &impl ControlActionSource,
	) -> (ControlGraphOutput, u128) {
		self.evaluate_inner::<true>(source)
	}

	pub fn active_node_ids(&self, mask: u128) -> impl Iterator<Item = &str> {
		self.order()
			.enumerate()
			.filter_map(move |(index, id)| (mask & (1_u128 << index) != 0).then_some(id))
	}

	pub fn input_kind(&self, action: &str) -> Option<InputActionKind> {
		self.steps.iter().find_map(|step| match &step.operation {
			CompiledOperation::Action(id, kind) if id == action => Some(*kind),
			_ => None,
		})
	}

	fn evaluate_inner<const TRACE: bool>(
		&self,
		source: &impl ControlActionSource,
	) -> (ControlGraphOutput, u128) {
		let mut output = ControlGraphOutput::default();
		let mut values = [Value::None; MAX_NODES];
		let mut active = 0_u128;
		for (index, step) in self.steps.iter().enumerate() {
			values[index] = match &step.operation {
				CompiledOperation::Action(id, kind) => match kind {
					InputActionKind::Button => Value::Button(source.button(id)),
					InputActionKind::Vector2 => Value::Vector(unit_vector(source.vector(id))),
					InputActionKind::Delta2 => {
						let mut look = source.look(id);
						look.delta = if look.delta.iter().all(|axis| axis.is_finite()) {
							look.delta.map(|axis| axis.clamp(-8.0, 8.0))
						} else {
							[0.0; 2]
						};
						look.rate = unit_vector(look.rate);
						Value::Look(look)
					}
				},
				CompiledOperation::Constant(value) => Value::Boolean(*value),
				CompiledOperation::ButtonHeld(input) => {
					Value::Boolean(matches!(values[*input], Value::Button(button) if button.held))
				}
				CompiledOperation::Not(input) => Value::Boolean(!values[*input].boolean()),
				CompiledOperation::And(left, right) => {
					Value::Boolean(values[*left].boolean() && values[*right].boolean())
				}
				CompiledOperation::Or(left, right) => {
					Value::Boolean(values[*left].boolean() || values[*right].boolean())
				}
				CompiledOperation::Gate(condition, input) => {
					if values[*condition].boolean() {
						values[*input]
					} else {
						Value::None
					}
				}
				CompiledOperation::Output(input, pin) => {
					match (pin, values[*input]) {
						(Output::Move, Value::Vector(value)) => output.intent.movement = value,
						(Output::Look, Value::Look(value)) => output.intent.look = value,
						(Output::Jump, Value::Button(value)) => output.intent.jump = value,
						(Output::Attack, Value::Button(value)) => output.intent.attack = value,
						(Output::AnimatorBool(parameter), value) => {
							output
								.presentation
								.insert(parameter.clone(), value.boolean());
						}
						_ => {}
					}
					Value::None
				}
			};
			if TRACE {
				let value = match step.operation {
					CompiledOperation::Output(input, _) => values[input],
					_ => values[index],
				};
				if value.active() {
					active |= 1_u128 << index;
				}
			}
		}
		(output, active)
	}
}

impl ControlGraph {
	pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
		if bytes.len() > MAX_CONTROL_GRAPH_BYTES {
			return Err("Controls Graph exceeds 128 KiB".into());
		}
		let graph: Self =
			serde_json::from_slice(bytes).map_err(|error| format!("Controls Graph: {error}"))?;
		graph.validate()?;
		Ok(graph)
	}

	pub fn to_json(&self) -> Result<Vec<u8>, String> {
		self.validate()?;
		let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
		if bytes.len() > MAX_CONTROL_GRAPH_BYTES {
			return Err("Formatted Controls Graph exceeds 128 KiB".into());
		}
		Ok(bytes)
	}

	pub fn validate(&self) -> Result<(), String> {
		self.compile_internal(None).map(|_| ())
	}

	pub fn compile(
		&self,
		actions: &BTreeMap<String, InputActionKind>,
		animator: Option<&AnimatorGraph>,
	) -> Result<CompiledControlGraph, String> {
		self.compile_internal(Some((actions, animator)))
	}

	fn compile_internal(
		&self,
		context: Option<(&BTreeMap<String, InputActionKind>, Option<&AnimatorGraph>)>,
	) -> Result<CompiledControlGraph, String> {
		if self.schema_version != 1 || !(1..=MAX_NODES).contains(&self.nodes.len()) {
			return Err("Controls Graph schema 1 requires 1–128 nodes".into());
		}
		let mut indices = BTreeMap::new();
		for (index, node) in self.nodes.iter().enumerate() {
			if !valid_game_action_id(&node.id) || indices.insert(node.id.as_str(), index).is_some()
			{
				return Err(format!(
					"Invalid or duplicate Controls Graph node ID {:?}",
					node.id
				));
			}
		}
		if self.layout.iter().any(|(id, position)| {
			!indices.contains_key(id.as_str())
				|| position.x.abs_diff(0) > 100_000
				|| position.y.abs_diff(0) > 100_000
		}) {
			return Err("Controls Graph layout has an invalid node or position".into());
		}
		let mut indegree = vec![0; self.nodes.len()];
		let mut followers = vec![Vec::new(); self.nodes.len()];
		for (index, node) in self.nodes.iter().enumerate() {
			for reference in node.data.references().into_iter().flatten() {
				let source = indices.get(reference).copied().ok_or_else(|| {
					format!(
						"Control node {:?} refers to missing node {reference:?}",
						node.id
					)
				})?;
				indegree[index] += 1;
				followers[source].push(index);
			}
		}
		let mut ready: BTreeSet<_> = indices
			.iter()
			.filter_map(|(&id, &index)| (indegree[index] == 0).then_some((id, index)))
			.collect();
		let mut compiled = BTreeMap::new();
		let mut steps = Vec::with_capacity(self.nodes.len());
		let mut outputs = BTreeSet::new();
		while let Some((id, index)) = ready.pop_first() {
			let (operation, result) =
				self.nodes[index].compile(&compiled, &mut outputs, context)?;
			compiled.insert(id, (steps.len(), result));
			steps.push(CompiledNode {
				id: id.to_owned(),
				operation,
			});
			for &next in &followers[index] {
				indegree[next] -= 1;
				if indegree[next] == 0 {
					ready.insert((self.nodes[next].id.as_str(), next));
				}
			}
		}
		if steps.len() != self.nodes.len() {
			let node = indices
				.iter()
				.find(|(_, index)| indegree[**index] > 0)
				.map(|(id, _)| *id)
				.unwrap_or("");
			return Err(format!("Controls Graph contains a cycle near {node:?}"));
		}
		if outputs.is_empty() {
			return Err("Controls Graph requires at least one intent or Animator output".into());
		}
		Ok(CompiledControlGraph { steps })
	}
}

impl ControlNodeData {
	fn references(&self) -> [Option<&str>; 2] {
		match self {
			Self::Action { .. } | Self::Boolean { .. } => [None, None],
			Self::And { left, right } | Self::Or { left, right } => [Some(left), Some(right)],
			Self::Gate { condition, input } => [Some(condition), Some(input)],
			Self::ButtonHeld { input }
			| Self::Not { input }
			| Self::Move { input }
			| Self::Look { input }
			| Self::Jump { input }
			| Self::Attack { input }
			| Self::AnimatorBool { input, .. } => [Some(input), None],
		}
	}
}

impl ControlNode {
	fn compile(
		&self,
		nodes: &BTreeMap<&str, (usize, Option<PortType>)>,
		outputs: &mut BTreeSet<Output>,
		context: Option<(&BTreeMap<String, InputActionKind>, Option<&AnimatorGraph>)>,
	) -> Result<(CompiledOperation, Option<PortType>), String> {
		let input = |reference: &str, expected: Option<PortType>| {
			let (index, actual) = nodes.get(reference).copied().ok_or_else(|| {
				format!(
					"Control node {:?} needs a value from {reference:?}",
					self.id
				)
			})?;
			let actual = actual.ok_or_else(|| {
				format!(
					"Control node {:?} cannot read from output {reference:?}",
					self.id
				)
			})?;
			if let Some(expected) = expected.filter(|&kind| kind != actual) {
				return Err(format!(
					"Control node {:?} has the wrong port type from {reference:?}: expected {expected:?}, got {actual:?}",
					self.id
				));
			}
			Ok((index, actual))
		};
		let resolve = |id: &str, kind: PortType| input(id, Some(kind)).map(|(index, _)| index);
		let mut output = |id: &str, kind: PortType, pin: Output| {
			let index = resolve(id, kind)?;
			if !outputs.insert(pin.clone()) {
				return Err(format!(
					"Control node {:?} duplicates output {pin:?}",
					self.id
				));
			}
			Ok((CompiledOperation::Output(index, pin), None))
		};
		use CompiledOperation as Op;
		use PortType::{Boolean, Button, Look, Vector};
		Ok(match &self.data {
			ControlNodeData::Action { action, value } => {
				if !valid_game_action_id(action)
					|| context.is_some_and(|(actions, _)| actions.get(action) != Some(value))
				{
					return Err(format!(
						"Control node {:?} references a missing or wrongly typed action {action:?}",
						self.id
					));
				}
				let port = match value {
					InputActionKind::Button => Button,
					InputActionKind::Vector2 => Vector,
					InputActionKind::Delta2 => Look,
				};
				(Op::Action(action.clone(), *value), Some(port))
			}
			ControlNodeData::Boolean { value } => (Op::Constant(*value), Some(Boolean)),
			ControlNodeData::ButtonHeld { input } => {
				(Op::ButtonHeld(resolve(input, Button)?), Some(Boolean))
			}
			ControlNodeData::Not { input } => (Op::Not(resolve(input, Boolean)?), Some(Boolean)),
			ControlNodeData::And { left, right } => (
				Op::And(resolve(left, Boolean)?, resolve(right, Boolean)?),
				Some(Boolean),
			),
			ControlNodeData::Or { left, right } => (
				Op::Or(resolve(left, Boolean)?, resolve(right, Boolean)?),
				Some(Boolean),
			),
			ControlNodeData::Gate {
				condition,
				input: value,
			} => {
				let (index, kind) = input(value, None)?;
				(Op::Gate(resolve(condition, Boolean)?, index), Some(kind))
			}
			ControlNodeData::Move { input } => return output(input, Vector, Output::Move),
			ControlNodeData::Look { input } => return output(input, Look, Output::Look),
			ControlNodeData::Jump { input } => return output(input, Button, Output::Jump),
			ControlNodeData::Attack { input } => return output(input, Button, Output::Attack),
			ControlNodeData::AnimatorBool { parameter, input } => {
				if !valid_game_action_id(parameter)
					|| context.is_some_and(|(_, animator)| {
						!animator.is_some_and(|graph| {
							graph.parameters.iter().any(|item| {
								item.id == *parameter
									&& matches!(
										item.source,
										AnimatorParameterSource::Boolean { .. }
									)
							})
						})
					}) {
					return Err(format!(
						"Control node {:?} requires an authored Animator Boolean parameter {parameter:?}",
						self.id
					));
				}
				return output(input, Boolean, Output::AnimatorBool(parameter.clone()));
			}
		})
	}
}

pub fn control_action_types(
	map: Option<&InputActionMap>,
	profile: &InputProfile,
) -> BTreeMap<String, InputActionKind> {
	if let Some(map) = map {
		return map
			.actions
			.iter()
			.map(|action| (action.id.clone(), action.value))
			.collect();
	}
	let mut actions: BTreeMap<_, _> = profile
		.actions
		.iter()
		.map(|action| (action.id.clone(), InputActionKind::Button))
		.collect();
	actions.insert("move".into(), InputActionKind::Vector2);
	actions.insert("look".into(), InputActionKind::Delta2);
	actions
}

fn unit_vector([x, y]: [f32; 2]) -> [f32; 2] {
	if !x.is_finite() || !y.is_finite() {
		return [0.0; 2];
	}
	let scale = f64::from(x).hypot(f64::from(y)).max(1.0);
	[(f64::from(x) / scale) as f32, (f64::from(y) / scale) as f32]
}
