use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

const MAX_NODES: usize = 128;
const MAX_CONNECTIONS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraph {
	pub version: u8,
	pub domain: String,
	pub nodes: Vec<MaterialGraphNode>,
	pub connections: Vec<MaterialGraphConnection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraphNode {
	pub id: String,
	pub kind: String,
	pub position: GraphPosition,
	pub values: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphPosition {
	pub x: f32,
	pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraphConnection {
	pub id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub kind: Option<String>,
	pub from: GraphEndpoint,
	pub to: GraphEndpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEndpoint {
	pub node: String,
	pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialGraphErrorCode {
	Document,
	Node,
	UnsupportedNode,
	Port,
	Connection,
	MissingInput,
	Cycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaterialGraphError {
	pub code: MaterialGraphErrorCode,
	pub node: Option<String>,
	pub port: Option<String>,
	pub message: &'static str,
}

impl MaterialGraphError {
	fn new(
		code: MaterialGraphErrorCode,
		node: Option<&str>,
		port: Option<&str>,
		message: &'static str,
	) -> Self {
		Self {
			code,
			node: node.map(str::to_owned),
			port: port.map(str::to_owned),
			message,
		}
	}

	fn at(code: MaterialGraphErrorCode, node: &str, message: &'static str) -> Self {
		Self::new(code, Some(node), None, message)
	}

	fn port(code: MaterialGraphErrorCode, endpoint: &GraphEndpoint, message: &'static str) -> Self {
		Self::new(code, Some(&endpoint.node), Some(&endpoint.port), message)
	}
}

impl fmt::Display for MaterialGraphError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if let Some(node) = &self.node {
			write!(f, "Material Graph node {node}")?;
			if let Some(port) = &self.port {
				write!(f, " ({port})")?;
			}
			write!(f, ": ")?;
		}
		f.write_str(self.message)
	}
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Input {
	Color,
	Factor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueType {
	Color,
	Scalar,
}

#[derive(Clone, Copy)]
enum Value {
	Color([f32; 3]),
	Scalar(f32),
}

#[derive(Clone, Copy)]
enum Node {
	Color([f32; 3]),
	Scalar(f32),
	Multiply(f32),
	Output,
}

impl Node {
	fn parse(source: &MaterialGraphNode) -> Result<Self, MaterialGraphError> {
		let bad_position = !source.position.x.is_finite() || !source.position.y.is_finite();
		if !valid_id(&source.id) || bad_position {
			return Err(MaterialGraphError::new(
				MaterialGraphErrorCode::Node,
				None,
				None,
				"Use a valid node ID and finite coordinates.",
			));
		}

		let value = |key: &'static str| {
			source
				.values
				.get(key)
				.copied()
				.filter(|value| (0.0..=1.0).contains(value))
				.ok_or_else(|| {
					MaterialGraphError::new(
						MaterialGraphErrorCode::Node,
						Some(&source.id),
						Some(key),
						"Enter a number within 0 to 1.",
					)
				})
		};
		let (node, field_count) = match source.kind.as_str() {
			"shader.color" => (
				Self::Color([value("red")?, value("green")?, value("blue")?]),
				3,
			),
			"value" => (Self::Scalar(value("value")?), 1),
			"shader.multiply" => (Self::Multiply(value("factor")?), 1),
			"shader.output" => (Self::Output, 0),
			_ => {
				return Err(MaterialGraphError::at(
					MaterialGraphErrorCode::UnsupportedNode,
					&source.id,
					"Only Color, Value, Multiply, and Material Output are supported.",
				));
			}
		};
		if source.values.len() != field_count {
			return Err(MaterialGraphError::at(
				MaterialGraphErrorCode::Node,
				&source.id,
				"Remove the unsupported node settings.",
			));
		}
		Ok(node)
	}

	fn input(self, port: &str) -> Option<(Input, ValueType)> {
		match (self, port) {
			(Self::Output, "color") | (Self::Multiply(_), "value") => {
				Some((Input::Color, ValueType::Color))
			}
			(Self::Multiply(_), "factor") => Some((Input::Factor, ValueType::Scalar)),
			_ => None,
		}
	}

	fn output(self, port: &str) -> Option<ValueType> {
		match (self, port) {
			(Self::Color(_), "color") | (Self::Multiply(_), "result") => Some(ValueType::Color),
			(Self::Scalar(_), "value") => Some(ValueType::Scalar),
			_ => None,
		}
	}

	fn color_input_port(self) -> &'static str {
		if matches!(self, Self::Output) {
			"color"
		} else {
			"value"
		}
	}
}

#[derive(Clone, Copy)]
enum Visit {
	Pending,
	Visiting,
	Done(Value),
}

struct Evaluation<'a> {
	graph: &'a MaterialGraph,
	nodes: Vec<Node>,
	sources: HashMap<(usize, Input), usize>,
	visits: Vec<Visit>,
}

impl MaterialGraph {
	pub fn retained_bytes(&self) -> usize {
		let mut bytes = std::mem::size_of::<Self>()
			.saturating_add(self.domain.capacity())
			.saturating_add(
				self.nodes
					.capacity()
					.saturating_mul(std::mem::size_of::<MaterialGraphNode>()),
			)
			.saturating_add(
				self.connections
					.capacity()
					.saturating_mul(std::mem::size_of::<MaterialGraphConnection>()),
			);
		for node in &self.nodes {
			bytes = bytes
				.saturating_add(node.id.capacity())
				.saturating_add(node.kind.capacity());
			for key in node.values.keys() {
				bytes = bytes.saturating_add(128).saturating_add(key.capacity());
			}
		}
		for edge in &self.connections {
			bytes = bytes
				.saturating_add(edge.id.capacity())
				.saturating_add(edge.kind.as_ref().map_or(0, String::capacity))
				.saturating_add(edge.from.node.capacity())
				.saturating_add(edge.from.port.capacity())
				.saturating_add(edge.to.node.capacity())
				.saturating_add(edge.to.port.capacity());
		}
		bytes
	}

	pub fn compile_base_color(&self) -> Result<[f32; 4], MaterialGraphError> {
		if self.version != 1 || self.domain != "shader" {
			return Err(document_error("Use version 1 of a material shader graph."));
		}
		if self.nodes.len() > MAX_NODES || self.connections.len() > MAX_CONNECTIONS {
			return Err(document_error("Use up to 128 nodes and 256 connections."));
		}

		let mut ids = HashMap::with_capacity(self.nodes.len());
		let mut nodes = Vec::with_capacity(self.nodes.len());
		let mut output = None;
		for source in &self.nodes {
			let node = Node::parse(source)?;
			let index = nodes.len();
			if ids.insert(source.id.as_str(), index).is_some() {
				return Err(MaterialGraphError::at(
					MaterialGraphErrorCode::Node,
					&source.id,
					"Node IDs must be unique.",
				));
			}
			if matches!(node, Node::Output) && output.replace(index).is_some() {
				return Err(MaterialGraphError::at(
					MaterialGraphErrorCode::Node,
					&source.id,
					"Keep only one Material Output node.",
				));
			}
			nodes.push(node);
		}
		let output = output.ok_or_else(|| document_error("Add a Material Output node."))?;

		let mut edges = HashSet::with_capacity(self.connections.len());
		let mut sources = HashMap::with_capacity(self.connections.len());
		for edge in &self.connections {
			if !valid_id(&edge.id)
				|| !edges.insert(&edge.id)
				|| edge.kind.as_deref().is_some_and(|kind| kind != "data")
			{
				return Err(document_error("Use uniquely identified data wires."));
			}
			if !valid_id(&edge.from.node)
				|| !valid_id(&edge.to.node)
				|| !valid_id(&edge.from.port)
				|| !valid_id(&edge.to.port)
			{
				return Err(document_error("Use valid IDs for both ends of each wire."));
			}
			let from = ids.get(edge.from.node.as_str()).copied().ok_or_else(|| {
				MaterialGraphError::port(
					MaterialGraphErrorCode::Connection,
					&edge.from,
					"The connected node is missing.",
				)
			})?;
			let to = ids.get(edge.to.node.as_str()).copied().ok_or_else(|| {
				MaterialGraphError::port(
					MaterialGraphErrorCode::Connection,
					&edge.to,
					"The connected node is missing.",
				)
			})?;
			let source_type = nodes[from].output(&edge.from.port).ok_or_else(|| {
				MaterialGraphError::port(
					MaterialGraphErrorCode::Port,
					&edge.from,
					"This output is unsupported.",
				)
			})?;
			let (input, target_type) = nodes[to].input(&edge.to.port).ok_or_else(|| {
				MaterialGraphError::port(
					MaterialGraphErrorCode::Port,
					&edge.to,
					"This input is unsupported.",
				)
			})?;
			if source_type != target_type || sources.insert((to, input), from).is_some() {
				return Err(MaterialGraphError::port(
					MaterialGraphErrorCode::Connection,
					&edge.to,
					"Connect one wire of the matching type.",
				));
			}
		}

		let mut evaluation = Evaluation {
			graph: self,
			nodes,
			sources,
			visits: vec![Visit::Pending; self.nodes.len()],
		};
		let Value::Color([red, green, blue]) = evaluation.evaluate(output)? else {
			return Err(document_error("Material Output requires a color."));
		};
		for index in 0..self.nodes.len() {
			evaluation.evaluate(index)?;
		}
		Ok([red, green, blue, 1.0])
	}
}

impl Evaluation<'_> {
	fn evaluate(&mut self, index: usize) -> Result<Value, MaterialGraphError> {
		match self.visits[index] {
			Visit::Done(value) => return Ok(value),
			Visit::Visiting => {
				return Err(MaterialGraphError::at(
					MaterialGraphErrorCode::Cycle,
					&self.graph.nodes[index].id,
					"Remove the cycle through this node.",
				));
			}
			Visit::Pending => self.visits[index] = Visit::Visiting,
		}
		let value = match self.nodes[index] {
			Node::Color(color) => Value::Color(color),
			Node::Scalar(value) => Value::Scalar(value),
			Node::Multiply(default) => {
				let factor = match self.sources.get(&(index, Input::Factor)).copied() {
					Some(source) => match self.evaluate(source)? {
						Value::Scalar(value) => value,
						Value::Color(_) => {
							return Err(document_error("Multiply factor requires a scalar."));
						}
					},
					None => default,
				};
				Value::Color(self.color(index)?.map(|channel| channel * factor))
			}
			Node::Output => Value::Color(self.color(index)?),
		};
		self.visits[index] = Visit::Done(value);
		Ok(value)
	}

	fn color(&mut self, index: usize) -> Result<[f32; 3], MaterialGraphError> {
		let source = self
			.sources
			.get(&(index, Input::Color))
			.copied()
			.ok_or_else(|| {
				MaterialGraphError::new(
					MaterialGraphErrorCode::MissingInput,
					Some(&self.graph.nodes[index].id),
					Some(self.nodes[index].color_input_port()),
					"Connect a color to this input.",
				)
			})?;
		match self.evaluate(source)? {
			Value::Color(color) => Ok(color),
			Value::Scalar(_) => Err(document_error("This input requires a color.")),
		}
	}
}

fn document_error(message: &'static str) -> MaterialGraphError {
	MaterialGraphError::new(MaterialGraphErrorCode::Document, None, None, message)
}

fn valid_id(id: &str) -> bool {
	!id.is_empty()
		&& id.len() <= 128
		&& id
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
