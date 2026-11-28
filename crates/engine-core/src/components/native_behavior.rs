use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBehaviorComponent {
	pub id: ComponentId,
	#[serde(default = "enabled")]
	pub enabled: bool,
	pub behavior: NativeBehaviorSpec,
}
fn enabled() -> bool {
	true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeBehaviorSpec {
	StateMachine {
		definition: crate::state_machine::MachineDef,
	},
	KeyDoor {
		actor: EntityKey,
		key_trigger: EntityKey,
		door: EntityKey,
		#[serde(default)]
		key_visual: Option<EntityKey>,
	},
	Collectible {
		actor: EntityKey,
		trigger: EntityKey,
		#[serde(default)]
		visual: Option<EntityKey>,
		#[serde(default)]
		despawn_owner: bool,
	},
	TimedSpawner {
		prefab_path: String,
		interval_ticks: u32,
		max_alive: u32,
		lifetime_ticks: u32,
		spawn_offset: [f32; 3],
		#[serde(default)]
		bindings: BTreeMap<String, EntityKey>,
	},
	Projectile {
		settings: ProjectileSettings,
	},
	ProjectileLauncher {
		settings: ProjectileLauncherSettings,
	},
}
impl NativeBehaviorSpec {
	pub fn entity_references(&self) -> Vec<&EntityKey> {
		match self {
			Self::StateMachine { definition } => definition.entity_references(),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				key_visual,
			} => std::iter::once(actor)
				.chain(std::iter::once(key_trigger))
				.chain(std::iter::once(door))
				.chain(key_visual.iter())
				.collect(),
			Self::Collectible {
				actor,
				trigger,
				visual,
				..
			} => std::iter::once(actor)
				.chain(std::iter::once(trigger))
				.chain(visual.iter())
				.collect(),
			Self::TimedSpawner { bindings, .. } => bindings.values().collect(),
			Self::Projectile { .. } | Self::ProjectileLauncher { .. } => Vec::new(),
		}
	}
	pub fn validate(&self) -> Result<(), String> {
		if let Self::StateMachine { definition } = self {
			// Validate shape before allocating the bounded reference traversal.
			return definition.validate();
		}
		for key in self.entity_references() {
			validate_reference(key)?;
		}
		match self {
			Self::StateMachine { definition } => definition.validate(),
			Self::Projectile { settings } => settings.validate(),
			Self::ProjectileLauncher { settings } => settings.validate(),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				..
			} if actor == key_trigger || actor == door || key_trigger == door => {
				Err("Key/door actor, trigger and door must be distinct entities".into())
			}
			Self::Collectible { actor, trigger, .. } if actor == trigger => {
				Err("Collectible actor and trigger must be distinct entities".into())
			}
			Self::TimedSpawner {
				prefab_path,
				interval_ticks,
				max_alive,
				lifetime_ticks,
				spawn_offset,
				bindings,
			} => {
				validate_model_path(prefab_path)?;
				if !prefab_path.to_ascii_lowercase().ends_with(".json")
					|| !(1..=1_000_000).contains(interval_ticks)
					|| !(1..=128).contains(max_alive)
					|| !(1..=1_000_000).contains(lifetime_ticks)
					|| !spawn_offset
						.iter()
						.all(|value| value.is_finite() && value.abs() <= 1_000_000.)
				{
					return Err("Spawner requires a relative JSON prefab, interval/lifetime 1–1000000 ticks, max_alive 1–128 and finite bounded offset".into());
				}
				if bindings.len() > 32
					|| bindings.keys().any(|name| {
						name.is_empty()
							|| name.len() > 64 || !name
							.bytes()
							.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
					}) {
					return Err("Spawner bindings require at most 32 named reference slots with bounded ASCII names".into());
				}
				Ok(())
			}
			_ => Ok(()),
		}
	}
	pub fn remap_entity_references(&mut self, remap: &HashMap<EntityKey, EntityKey>) {
		fn one(key: &mut EntityKey, remap: &HashMap<EntityKey, EntityKey>) {
			if let Some(next) = remap.get(key) {
				*key = next.clone();
			}
		}
		match self {
			Self::StateMachine { definition } => definition.remap(remap),
			Self::KeyDoor {
				actor,
				key_trigger,
				door,
				key_visual,
			} => {
				one(actor, remap);
				one(key_trigger, remap);
				one(door, remap);
				if let Some(key) = key_visual {
					one(key, remap);
				}
			}
			Self::Collectible {
				actor,
				trigger,
				visual,
				..
			} => {
				one(actor, remap);
				one(trigger, remap);
				if let Some(key) = visual {
					one(key, remap);
				}
			}
			Self::TimedSpawner { bindings, .. } => {
				for key in bindings.values_mut() {
					one(key, remap);
				}
			}
			Self::Projectile { .. } | Self::ProjectileLauncher { .. } => {}
		}
	}
}
fn validate_reference(key: &EntityKey) -> Result<(), String> {
	key.validate()?;
	if key.as_str().len() > 256 || key.as_str().chars().any(char::is_control) {
		return Err("Component entity reference exceeds its key budget".into());
	}
	Ok(())
}
