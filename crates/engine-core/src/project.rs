//! Authored player configuration. Behavior targets use persistent scene keys;
//! no engine policy depends on demo roles, keyboard mappings, or editor layout.
use std::{
	collections::{HashMap, HashSet},
	fs::File,
	io::{Read, Write},
	path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
	EntityKey,
	scene_catalog::{SceneCatalog, SceneDescriptor, SceneId},
	scene_data::SceneData,
};

pub const PROJECT_SCHEMA_VERSION: u32 = 3;
pub const MAX_PROJECT_BYTES: usize = 1024 * 1024;
pub const MAX_BEHAVIORS: usize = 1024;
pub const MAX_BEHAVIOR_DEFINITIONS: usize = 1024;

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
	Shared {
		entity: EntityKey,
		definition: String,
	},
}

impl BehaviorSpec {
	pub fn entity(&self) -> &EntityKey {
		match self {
			Self::ControlledMove { entity, .. }
			| Self::Spin { entity, .. }
			| Self::Shared { entity, .. } => entity,
		}
	}

	pub fn validate(&self) -> Result<(), String> {
		let entity = self.entity().as_str();

		if entity.is_empty()
			|| entity.len() > 256
			|| entity.chars().any(|c| c.is_whitespace() || c.is_control())
		{
			return Err("Behavior target must be a valid persistent entity key".into());
		}

		match self {
			Self::ControlledMove { speed, space, .. } => BehaviorSettings::ControlledMove {
				speed: *speed,
				space: *space,
			}
			.validate(),
			Self::Spin {
				angular_velocity, ..
			} => BehaviorSettings::Spin {
				angular_velocity: *angular_velocity,
			}
			.validate(),
			Self::Shared { definition, .. } => validate_definition_id(definition),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneBehavior {
	pub scene_id: SceneId,
	pub behavior: BehaviorSpec,
}
impl SceneBehavior {
	pub fn entity(&self) -> &EntityKey {
		self.behavior.entity()
	}
	pub fn validate(&self) -> Result<(), String> {
		self.behavior.validate()
	}
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
		};
		project.validate_scene(scene)?;
		Ok(project)
	}

	pub fn validate(&self) -> Result<(), String> {
		self.validated_definitions().map(|_| ())
	}

	fn validated_definitions(&self) -> Result<HashMap<&str, &BehaviorSettings>, String> {
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
		self.scenes
			.get(&self.startup_scene_id)
			.map_err(|e| e.to_string())?;
		for entry in self.scenes.iter() {
			SceneDescriptor::new(entry.id().clone(), entry.asset_path())?;
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
		if self.behavior_definitions.len() > MAX_BEHAVIOR_DEFINITIONS {
			return Err(format!(
				"Project exceeds {MAX_BEHAVIOR_DEFINITIONS} behavior definitions"
			));
		}
		let mut definitions = HashMap::with_capacity(self.behavior_definitions.len());
		for (index, definition) in self.behavior_definitions.iter().enumerate() {
			definition
				.validate()
				.map_err(|error| format!("Behavior definition {index}: {error}"))?;
			if definitions
				.insert(definition.id.as_str(), &definition.settings)
				.is_some()
			{
				return Err(format!(
					"Duplicate behavior definition ID {:?}",
					definition.id
				));
			}
		}
		for (index, behavior) in self.behaviors.iter().enumerate() {
			self.scenes
				.get(&behavior.scene_id)
				.map_err(|e| e.to_string())?;
			behavior
				.validate()
				.map_err(|error| format!("Behavior {index}: {error}"))?;
			if let BehaviorSpec::Shared { definition, .. } = &behavior.behavior
				&& !definitions.contains_key(definition.as_str())
			{
				return Err(format!(
					"Behavior {index} references unknown behavior definition {definition:?}"
				));
			}
		}
		serde_json::to_writer(SizeBudget(0), self)
			.map_err(|error| format!("Project size validation failed: {error}"))?;
		Ok(definitions)
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
	pub fn behaviors_for<'a>(
		&'a self,
		id: &'a SceneId,
	) -> impl Iterator<Item = &'a BehaviorSpec> + 'a {
		self.behaviors
			.iter()
			.filter(move |b| &b.scene_id == id)
			.map(|b| &b.behavior)
	}
	pub fn resolve_behavior(&self, behavior: &BehaviorSpec) -> Result<ResolvedBehavior, String> {
		behavior.validate()?;
		let settings = match behavior {
			BehaviorSpec::ControlledMove { speed, space, .. } => BehaviorSettings::ControlledMove {
				speed: *speed,
				space: *space,
			},
			BehaviorSpec::Spin {
				angular_velocity, ..
			} => BehaviorSettings::Spin {
				angular_velocity: *angular_velocity,
			},
			BehaviorSpec::Shared { definition, .. } => {
				let mut definitions = self
					.behavior_definitions
					.iter()
					.filter(|candidate| &candidate.id == definition);
				let shared = definitions
					.next()
					.ok_or_else(|| format!("Unknown behavior definition {definition:?}"))?;
				if definitions.next().is_some() {
					return Err(format!("Duplicate behavior definition ID {definition:?}"));
				}
				shared.validate()?;
				shared.settings.clone()
			}
		};
		Ok(ResolvedBehavior {
			entity: behavior.entity().clone(),
			settings,
		})
	}
	pub fn resolved_behaviors_for(
		&self,
		scene_id: &SceneId,
	) -> Result<Vec<ResolvedBehavior>, String> {
		let definitions = self.validated_definitions()?;
		self.scenes
			.get(scene_id)
			.map_err(|error| error.to_string())?;
		Ok(self
			.behaviors_for(scene_id)
			.map(|behavior| {
				let settings = match behavior {
					BehaviorSpec::ControlledMove { speed, space, .. } => {
						BehaviorSettings::ControlledMove {
							speed: *speed,
							space: *space,
						}
					}
					BehaviorSpec::Spin {
						angular_velocity, ..
					} => BehaviorSettings::Spin {
						angular_velocity: *angular_velocity,
					},
					BehaviorSpec::Shared { definition, .. } => {
						// The index and every reference were validated together above.
						(*definitions[definition.as_str()]).clone()
					}
				};
				ResolvedBehavior {
					entity: behavior.entity().clone(),
					settings,
				}
			})
			.collect())
	}
	pub fn add_behavior(&mut self, behavior: BehaviorSpec) {
		self.behaviors.push(SceneBehavior {
			scene_id: self.startup_scene_id.clone(),
			behavior,
		});
	}
	pub fn validate_scene(&self, scene: &SceneData) -> Result<(), String> {
		self.validate_scene_for(&self.startup_scene_id, scene)
	}
	pub fn validate_scene_for(&self, id: &SceneId, scene: &SceneData) -> Result<(), String> {
		self.validate()?;
		self.scenes.get(id).map_err(|e| e.to_string())?;
		scene.validate()?;
		let keys: HashSet<_> = scene
			.entities
			.iter()
			.map(|entity| entity.key.as_str())
			.collect();
		for (index, behavior) in self.behaviors_for(id).enumerate() {
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
		let header: crate::project_format::VersionHeader =
			serde_json::from_str(source).map_err(|e| format!("Invalid project JSON: {e}"))?;
		if header.schema_version != 2 && header.schema_version != PROJECT_SCHEMA_VERSION {
			return Err(format!(
				"Unsupported project schema version: {}",
				header.schema_version
			));
		}
		let mut value: Self = serde_json::from_str(source)
			.map_err(|error| format!("Invalid project JSON: {error}"))?;
		if header.schema_version == 2 {
			let fields: serde_json::Value = serde_json::from_str(source)
				.map_err(|error| format!("Invalid project JSON: {error}"))?;
			if fields.get("behavior_definitions").is_some()
				|| value
					.behaviors
					.iter()
					.any(|entry| matches!(&entry.behavior, BehaviorSpec::Shared { .. }))
			{
				return Err(
					"Shared behaviors and behavior_definitions require project schema version 3"
						.into(),
				);
			}
			value.schema_version = PROJECT_SCHEMA_VERSION;
		}
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BehaviorSettings {
	ControlledMove {
		speed: f32,
		#[serde(default)]
		space: BehaviorSpace,
	},
	Spin {
		angular_velocity: [f32; 3],
	},
}

impl BehaviorSettings {
	pub fn validate(&self) -> Result<(), String> {
		match self {
			Self::ControlledMove { speed, .. } => {
				if !speed.is_finite() || !(0.0..=1_000_000.0).contains(speed) {
					return Err("Movement speed must be finite and within 0..=1000000".into());
				}
			}
			Self::Spin { angular_velocity } => {
				if angular_velocity
					.iter()
					.any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
				{
					return Err("Angular velocity must be finite and within ±1000000".into());
				}
			}
		}

		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorDefinition {
	pub id: String,
	pub settings: BehaviorSettings,
}

impl BehaviorDefinition {
	pub fn validate(&self) -> Result<(), String> {
		validate_definition_id(&self.id)?;
		self.settings.validate()
	}
}

fn validate_definition_id(id: &str) -> Result<(), String> {
	if id.is_empty() || id.len() > 256 || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
		return Err("Behavior definition ID must be nonempty, at most 256 bytes, and contain no whitespace or control characters".into());
	}
	Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBehavior {
	pub entity: EntityKey,
	pub settings: BehaviorSettings,
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
