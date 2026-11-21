//! Authored player configuration. Behavior targets use persistent scene keys;
//! no engine policy depends on demo roles, keyboard mappings, or editor layout.
mod behavior;
use crate::project_format::SizeBudget;
pub use behavior::{
	BehaviorDefinition, BehaviorSettings, BehaviorSpace, BehaviorSpec, ResolvedBehavior,
	SceneBehavior,
};
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
	EntityKey,
	input::InputProfile,
	scene_catalog::{SceneCatalog, SceneDescriptor, SceneId},
	scene_data::SceneData,
};

pub const PROJECT_SCHEMA_VERSION: u32 = 3;
pub const MAX_PROJECT_BYTES: usize = 1024 * 1024;
pub const MAX_BEHAVIORS: usize = 1024;
pub const MAX_BEHAVIOR_DEFINITIONS: usize = 1024;

/// Shared traversal budget for opaque native-module settings. Run this before
/// cloning/serializing a direct DTO as well as before accepting a project.
pub fn validate_gameplay_payload(settings: &serde_json::Value) -> Result<(), String> {
	let mut pending = vec![(settings, 0usize)];
	let mut nodes = 0usize;
	let mut bytes = 0usize;
	while let Some((value, depth)) = pending.pop() {
		nodes += 1;
		if depth > 16 || nodes > 16_384 || bytes > 256 * 1024 {
			return Err("Gameplay settings exceed their size or nesting budget".into());
		}
		match value {
			serde_json::Value::Object(fields) => {
				if fields.len() + pending.len() > 16_384 - nodes {
					return Err("Gameplay settings exceed their node budget".into());
				}
				for (key, value) in fields {
					bytes = bytes.saturating_add(key.len());
					pending.push((value, depth + 1));
				}
			}
			serde_json::Value::Array(values) => {
				if values.len() + pending.len() > 16_384 - nodes {
					return Err("Gameplay settings exceed their node budget".into());
				}
				pending.extend(values.iter().map(|value| (value, depth + 1)));
			}
			serde_json::Value::String(value) => bytes = bytes.saturating_add(value.len()),
			_ => {}
		}
	}
	if bytes > 256 * 1024 {
		return Err("Gameplay settings exceed their size budget".into());
	}
	Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectData {
	pub schema_version: u32,
	pub name: String,
	pub scenes: SceneCatalog,
	pub startup_scene_id: SceneId,
	#[serde(default = "default_hz")]
	pub fixed_hz: u32,
	#[serde(default = "default_catchup")]
	pub max_catchup_steps: u32,
	#[serde(default)]
	pub behaviors: Vec<SceneBehavior>,
	#[serde(default)]
	pub behavior_definitions: Vec<BehaviorDefinition>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub input_profile: Option<InputProfile>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub gameplay_module: Option<String>,
	/// Native modules own the typed schema. Core bounds the generic payload.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub gameplay_settings: Option<serde_json::Value>,
}

fn default_hz() -> u32 {
	60
}
fn default_catchup() -> u32 {
	8
}

impl ProjectData {
	/// A new authored project starts with no implicit game behavior. Applications
	/// may explicitly provide migration/demo behavior through `behaviors`.
	pub fn for_scene(
		name: impl Into<String>,
		startup_scene: impl Into<String>,
		scene: &SceneData,
	) -> Result<Self, String> {
		Self::for_scene_with_id(name, SceneId::generate(), startup_scene, scene)
	}

	/// Supply an existing identity, or a stable identity for a transient tool
	/// document. Newly authored scenes normally use `for_scene` instead.
	pub fn for_scene_with_id(
		name: impl Into<String>,
		id: SceneId,
		startup_scene: impl Into<String>,
		scene: &SceneData,
	) -> Result<Self, String> {
		let mut scenes = SceneCatalog::default();
		scenes
			.insert(SceneDescriptor::new(id.clone(), startup_scene)?)
			.map_err(|e| e.to_string())?;
		let project = Self {
			schema_version: PROJECT_SCHEMA_VERSION,
			name: name.into(),
			scenes,
			startup_scene_id: id,
			fixed_hz: default_hz(),
			max_catchup_steps: default_catchup(),
			behaviors: Vec::new(),
			behavior_definitions: Vec::new(),
			input_profile: None,
			gameplay_module: None,
			gameplay_settings: None,
		};
		project.validate_scene(scene)?;
		Ok(project)
	}

	pub fn validate(&self) -> Result<(), String> {
		self.validated_definitions().map(|_| ())
	}

	pub fn startup_scene_path(&self) -> Result<&str, String> {
		self.scene_path(&self.startup_scene_id)
	}
	pub fn scene_path(&self, id: &SceneId) -> Result<&str, String> {
		self.scenes
			.get(id)
			.map(|s| s.asset_path())
			.map_err(|e| e.to_string())
	}
}
