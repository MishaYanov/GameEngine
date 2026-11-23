use super::*;
use crate::input::Key;

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
	AcceptAction {
		entity: EntityKey,
		action: String,
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
			| Self::AcceptAction { entity, .. }
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
			Self::AcceptAction { action, .. } => validate_action(action),
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
	AcceptAction {
		action: String,
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
			Self::AcceptAction { action } => validate_action(action)?,
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

fn validate_action(action: &str) -> Result<(), String> {
	if !crate::input::valid_game_action_id(action) {
		return Err("Accept Action requires a valid game action ID".into());
	}
	Ok(())
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

impl ProjectData {
	pub(super) fn validated_definitions(&self) -> Result<HashMap<&str, &BehaviorSettings>, String> {
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
		if let Some(profile) = &self.input_profile {
			profile.validate()?;
			if profile.movement.contains_key(Key::Escape) {
				return Err("Escape is reserved for leaving game input".into());
			}
		}
		if let Some(physics_layers) = &self.physics_layers {
			physics_layers.validate()?;
		}
		if let Some(id) = &self.gameplay_module {
			let mut bytes = id.bytes();
			let valid = id.len() <= 32
				&& bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
				&& bytes
					.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');

			if !valid {
				return Err("Gameplay module ID must start with a lowercase ASCII letter and contain at most 32 lowercase ASCII letters, digits, or underscores".into());
			}
		}
		let mut definitions = HashMap::with_capacity(self.behavior_definitions.len());
		if let Some(settings) = &self.gameplay_settings {
			validate_gameplay_payload(settings)?;
		}
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
			BehaviorSpec::AcceptAction { action, .. } => BehaviorSettings::AcceptAction {
				action: action.clone(),
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
					BehaviorSpec::AcceptAction { action, .. } => BehaviorSettings::AcceptAction {
						action: action.clone(),
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
		let player_actors: HashSet<_> = scene
			.entities
			.iter()
			.filter(|entity| {
				entity
					.player_controller
					.as_ref()
					.is_some_and(|controller| controller.enabled)
			})
			.map(|entity| &entity.key)
			.collect();
		for (index, behavior) in self.behaviors_for(id).enumerate() {
			if !keys.contains(behavior.entity().as_str()) {
				return Err(format!(
					"Behavior {index} targets missing entity {:?}",
					behavior.entity().as_str()
				));
			}
			if player_actors.contains(behavior.entity())
				&& !matches!(
					self.resolve_behavior(behavior)?.settings,
					BehaviorSettings::AcceptAction { .. }
				) {
				return Err("Player Controller owns this Character Controller's body; remove its configured movement or spin behavior".into());
			}
		}
		if let Some(actor) = scene.local_player() {
			let jump = &actor
				.character_controller
				.as_ref()
				.expect("validated local controller")
				.settings
				.jump_action;
			if self
				.input_profile
				.as_ref()
				.is_none_or(|profile| !profile.actions.iter().any(|action| &action.id == jump))
			{
				return Err(format!(
					"Local Player jump action {jump:?} is absent from Game Input"
				));
			}
		}
		Ok(())
	}
}
