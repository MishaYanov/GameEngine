//! Authored player configuration. Behavior targets use persistent scene keys;
//! no engine policy depends on demo roles, keyboard mappings, or editor layout.
use std::{
	collections::HashSet,
	fs::File,
	io::{Read, Write},
	path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
	EntityKey,
	scene_data::{SceneData, validate_model_path},
};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROJECT_BYTES: usize = 1024 * 1024;
pub const MAX_BEHAVIORS: usize = 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorSpace {
	Local,
	#[default]
	World,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BehaviorSpec {
	ControlledMove {
		entity: EntityKey,
		speed: f32,
		#[serde(default)]
		space: BehaviorSpace,
	},
	Spin {
		entity: EntityKey,
		angular_velocity: [f32; 3],
	},
}

impl BehaviorSpec {
	pub fn entity(&self) -> &EntityKey {
		match self {
			Self::ControlledMove { entity, .. } | Self::Spin { entity, .. } => entity,
		}
	}
	pub fn validate(&self) -> Result<(), String> {
		let entity = self.entity();
		if entity.as_str().is_empty()
			|| entity.as_str().len() > 256
			|| entity
				.as_str()
				.chars()
				.any(|c| c.is_whitespace() || c.is_control())
		{
			return Err("Behavior target must be a valid persistent entity key".into());
		}
		match self {
			Self::ControlledMove { speed, .. }
				if !speed.is_finite() || !(0.0..=1_000_000.0).contains(speed) =>
			{
				Err("Controlled movement speed must be finite and within 0..=1000000".into())
			}
			Self::Spin {
				angular_velocity, ..
			} if angular_velocity
				.iter()
				.any(|v| !v.is_finite() || v.abs() > 1_000_000.0) =>
			{
				Err(
					"Spin angular velocity must contain finite radians per second within ±1000000"
						.into(),
				)
			}
			_ => Ok(()),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectData {
	pub schema_version: u32,
	pub name: String,
	pub startup_scene: String,
	#[serde(default = "default_hz")]
	pub fixed_hz: u32,
	#[serde(default = "default_catchup")]
	pub max_catchup_steps: u32,
	#[serde(default)]
	pub behaviors: Vec<BehaviorSpec>,
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
		let project = Self {
			schema_version: PROJECT_SCHEMA_VERSION,
			name: name.into(),
			startup_scene: startup_scene.into(),
			fixed_hz: default_hz(),
			max_catchup_steps: default_catchup(),
			behaviors: Vec::new(),
		};
		project.validate_scene(scene)?;
		Ok(project)
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != PROJECT_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported project schema version: {}",
				self.schema_version
			));
		}
		if self.name.trim().is_empty()
			|| self.name.len() > 80
			|| self.name.chars().any(char::is_control)
		{
			return Err("Project name must contain visible text, at most 80 bytes, and no control characters".into());
		}
		validate_model_path(&self.startup_scene)
			.map_err(|error| format!("Startup scene: {error}"))?;
		if !self.startup_scene.to_ascii_lowercase().ends_with(".json") {
			return Err("Startup scene must name an asset-relative JSON scene".into());
		}
		if self.fixed_hz == 0
			|| self.fixed_hz > 1000
			|| self.max_catchup_steps == 0
			|| self.max_catchup_steps > 8
		{
			return Err(
				"Project fixed_hz must be 1..=1000 and max_catchup_steps must be 1..=8".into(),
			);
		}
		if self.behaviors.len() > MAX_BEHAVIORS {
			return Err(format!("Project exceeds {MAX_BEHAVIORS} behaviors"));
		}
		for (index, behavior) in self.behaviors.iter().enumerate() {
			behavior
				.validate()
				.map_err(|error| format!("Behavior {index}: {error}"))?;
		}
		serde_json::to_writer(SizeBudget(0), self)
			.map_err(|error| format!("Project size validation failed: {error}"))?;
		Ok(())
	}

	pub fn validate_scene(&self, scene: &SceneData) -> Result<(), String> {
		self.validate()?;
		scene.validate()?;
		let keys: HashSet<_> = scene
			.entities
			.iter()
			.map(|entity| entity.key.as_str())
			.collect();
		for (index, behavior) in self.behaviors.iter().enumerate() {
			if !keys.contains(behavior.entity().as_str()) {
				return Err(format!(
					"Behavior {index} targets missing entity {:?}",
					behavior.entity().as_str()
				));
			}
		}
		Ok(())
	}

	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_PROJECT_BYTES {
			return Err("Project JSON exceeds 1 MiB".into());
		}
		let value: Self = serde_json::from_str(source)
			.map_err(|error| format!("Invalid project JSON: {error}"))?;
		value.validate()?;
		Ok(value)
	}

	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let source = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
		if source.len() > MAX_PROJECT_BYTES {
			return Err("Formatted project JSON exceeds 1 MiB".into());
		}
		Ok(source)
	}

	pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
		let file =
			File::open(path.as_ref()).map_err(|error| format!("Cannot read project: {error}"))?;
		let metadata = file.metadata().map_err(|error| error.to_string())?;
		if !metadata.is_file() {
			return Err("Project manifest must be a regular file".into());
		}
		if metadata.len() > MAX_PROJECT_BYTES as u64 {
			return Err("Project JSON exceeds 1 MiB".into());
		}
		let mut source = String::new();
		file.take(MAX_PROJECT_BYTES as u64 + 1)
			.read_to_string(&mut source)
			.map_err(|error| error.to_string())?;
		Self::from_json(&source)
	}
}

struct SizeBudget(usize);
impl Write for SizeBudget {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		self.0 = self
			.0
			.checked_add(bytes.len())
			.ok_or_else(|| std::io::Error::other("Project size overflow"))?;
		if self.0 > MAX_PROJECT_BYTES {
			return Err(std::io::Error::other("Project JSON exceeds 1 MiB"));
		}
		Ok(bytes.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}
