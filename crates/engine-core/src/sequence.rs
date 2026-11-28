//! Reusable cinematic timelines and scene-local bindings.
use crate::{
	EntityKey, SceneData,
	scene_catalog::{SceneCatalog, SceneId},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_SEQUENCES: usize = 64;
const MAX_INSTANCES: usize = 256;
const MAX_CUES: usize = 256;
const MAX_DURATION_MS: u32 = 10 * 60 * 1_000;
const MAX_SUBTITLE_BYTES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceLibrary {
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub assets: Vec<SequenceAsset>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub instances: Vec<SequenceInstance>,
}

impl SequenceLibrary {
	pub fn is_empty(&self) -> bool {
		self.assets.is_empty() && self.instances.is_empty()
	}

	pub fn asset(&self, id: &EntityKey) -> Option<&SequenceAsset> {
		self.assets.iter().find(|asset| &asset.id == id)
	}

	pub fn instance_for(&self, scene: &SceneId, id: &EntityKey) -> Option<&SequenceInstance> {
		self.instances
			.iter()
			.find(|instance| &instance.scene_id == scene && &instance.id == id)
	}

	pub fn validate_catalog(&self, scenes: &SceneCatalog) -> Result<(), String> {
		if self.assets.len() > MAX_SEQUENCES || self.instances.len() > MAX_INSTANCES {
			return Err("Project exceeds its sequence asset or instance limit".into());
		}
		let mut assets = HashSet::new();
		for asset in &self.assets {
			if !assets.insert(&asset.id) {
				return Err(format!("Duplicate sequence asset {}", asset.id.as_str()));
			}
			asset.validate()?;
		}
		let mut instances = HashSet::new();
		for instance in &self.instances {
			validate_key(&instance.id)?;
			if !instances.insert((&instance.scene_id, &instance.id)) {
				return Err(format!(
					"Duplicate sequence instance {}",
					instance.id.as_str()
				));
			}
			scenes.get(&instance.scene_id).map_err(|_| {
				format!(
					"Sequence instance {} uses a missing scene",
					instance.id.as_str()
				)
			})?;
			let asset = self.asset(&instance.sequence_id).ok_or_else(|| {
				format!(
					"Sequence instance {} uses a missing asset",
					instance.id.as_str()
				)
			})?;
			instance.validate_roles(asset)?;
		}
		Ok(())
	}

	/// Verify authored components without loading renderer resources.
	pub fn validate_scene(&self, scene: &SceneId, data: &SceneData) -> Result<(), String> {
		let mut instances = self
			.instances
			.iter()
			.filter(|instance| &instance.scene_id == scene)
			.peekable();
		if instances.peek().is_none() {
			return Ok(());
		}
		let entities: HashMap<_, _> = data
			.entities
			.iter()
			.map(|entity| (&entity.key, entity))
			.collect();
		for instance in instances {
			let asset = self
				.asset(&instance.sequence_id)
				.ok_or("Sequence asset is missing")?;
			instance.validate_roles(asset)?;
			let entity = |key: &EntityKey| {
				entities
					.get(key)
					.copied()
					.filter(|entity| entity.active)
					.ok_or_else(|| {
						format!(
							"Sequence instance {} requires active entity {}",
							instance.id.as_str(),
							key.as_str()
						)
					})
			};
			for cue in &asset.cues {
				let Some(role) = cue.action.role() else {
					continue;
				};
				let actor = entity(&instance.bindings[role])?;
				let compatible = match &cue.action {
					SequenceAction::CameraCut { .. } => {
						actor.camera.as_ref().is_some_and(|camera| camera.enabled)
					}
					SequenceAction::PlayClip { .. } => {
						actor.animation.as_ref().is_some_and(|clip| clip.enabled)
							|| actor.animator.as_ref().is_some_and(|animator| {
							animator.enabled
								&& matches!(
										animator.settings.binding,
										crate::animation::AnimatorBinding::Imported { .. }
									)
						})
					}
					SequenceAction::Audio { .. } => actor
						.audio_source
						.as_ref()
						.is_some_and(|source| source.enabled && source.settings.clip.is_some()),
					SequenceAction::Subtitle { .. } => unreachable!("Subtitle has no binding"),
				};
				if !compatible {
					return Err(format!(
						"Sequence instance {} has an incompatible target for role {}",
						instance.id.as_str(),
						role.as_str()
					));
				}
			}
			if let Some(trigger) = &instance.trigger {
				let sensor = entity(&trigger.sensor)?;
				entity(&trigger.actor)?;
				if !sensor.collision_enabled
					|| !sensor
					.collider
					.as_ref()
					.is_some_and(|collider| collider.interaction().sensor)
					|| trigger.sensor == trigger.actor
				{
					return Err(format!(
						"Sequence instance {} requires a distinct enabled trigger sensor",
						instance.id.as_str()
					));
				}
			}
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceAsset {
	pub id: EntityKey,
	#[serde(default = "sequence_schema_version")]
	pub schema_version: u32,
	pub duration_ms: u32,
	pub cues: Vec<SequenceCue>,
}

impl SequenceAsset {
	fn validate(&self) -> Result<(), String> {
		validate_key(&self.id)?;
		if self.schema_version != sequence_schema_version()
			|| !(1..=MAX_DURATION_MS).contains(&self.duration_ms)
			|| !(1..=MAX_CUES).contains(&self.cues.len())
		{
			return Err(format!(
				"Sequence {} has invalid version, duration or cue count",
				self.id.as_str()
			));
		}
		let mut previous = 0;
		for cue in &self.cues {
			if cue.at_ms < previous || cue.at_ms >= self.duration_ms {
				return Err(format!(
					"Sequence {} cues must be ordered within its duration",
					self.id.as_str()
				));
			}
			previous = cue.at_ms;
			if let Some(role) = cue.action.role() {
				validate_key(role)?;
			}
			if let SequenceAction::Subtitle { text, duration_ms } = &cue.action
				&& (text.trim().is_empty()
				|| text.len() > MAX_SUBTITLE_BYTES
				|| text.chars().any(char::is_control)
				|| *duration_ms == 0
				|| *duration_ms > self.duration_ms - cue.at_ms)
			{
				return Err(format!(
					"Sequence {} has an invalid subtitle",
					self.id.as_str()
				));
			}
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceInstance {
	pub id: EntityKey,
	pub scene_id: SceneId,
	pub sequence_id: EntityKey,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub bindings: BTreeMap<EntityKey, EntityKey>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub trigger: Option<SequenceTrigger>,
	#[serde(default = "default_once_per_entry")]
	pub once_per_entry: bool,
}

impl SequenceInstance {
	fn validate_roles(&self, asset: &SequenceAsset) -> Result<(), String> {
		let roles: HashSet<_> = asset
			.cues
			.iter()
			.filter_map(|cue| cue.action.role())
			.collect();
		if self.bindings.len() != roles.len()
			|| roles.iter().any(|role| !self.bindings.contains_key(*role))
		{
			return Err(format!(
				"Sequence instance {} must bind exactly its cue roles",
				self.id.as_str()
			));
		}
		for key in self.bindings.values() {
			validate_key(key)?;
		}
		if let Some(trigger) = &self.trigger {
			validate_key(&trigger.sensor)?;
			validate_key(&trigger.actor)?;
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceTrigger {
	pub sensor: EntityKey,
	pub actor: EntityKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceCue {
	pub at_ms: u32,
	pub action: SequenceAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SequenceAction {
	CameraCut { role: EntityKey },
	PlayClip { role: EntityKey, clip_index: u32 },
	Audio { role: EntityKey },
	Subtitle { text: String, duration_ms: u32 },
}

impl SequenceAction {
	pub(crate) fn role(&self) -> Option<&EntityKey> {
		match self {
			Self::CameraCut { role } | Self::PlayClip { role, .. } | Self::Audio { role } => {
				Some(role)
			}
			Self::Subtitle { .. } => None,
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequencePresentation {
	pub instance: EntityKey,
	pub subtitle: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SequenceEvent {
	Started {
		instance: EntityKey,
		tick: u64,
	},
	Finished {
		instance: EntityKey,
		tick: u64,
	},
	Skipped {
		instance: EntityKey,
		tick: u64,
	},
	Cancelled {
		instance: EntityKey,
		tick: u64,
	},
	Subtitle {
		instance: EntityKey,
		tick: u64,
		text: String,
		duration_ms: u32,
	},
}

fn sequence_schema_version() -> u32 {
	1
}

fn default_once_per_entry() -> bool {
	true
}

fn validate_key(key: &EntityKey) -> Result<(), String> {
	key.validate()?;
	if key.as_str().len() > 128 || key.as_str().chars().any(char::is_control) {
		return Err(
			"Sequence IDs and bindings must be at most 128 bytes and have no control characters"
				.into(),
		);
	}
	Ok(())
}
