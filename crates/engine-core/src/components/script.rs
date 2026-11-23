//! Saved, bounded settings and reference bindings for a compiled object script.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{EntityKey, scene_data::EntityData};

use super::ComponentId;

pub const MAX_SCRIPTS_PER_OBJECT: usize = 8;
const MAX_ENTRIES: usize = 32;
const MAX_PROPERTY_TEXT_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptProperty {
	Boolean(bool),
	Number(f32),
	Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ScriptReference {
	Entity { entity: EntityKey },
	Slot { slot: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptSettings {
	pub script_id: String,
	pub config_version: u32,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub properties: BTreeMap<String, ScriptProperty>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub references: BTreeMap<String, ScriptReference>,
}

impl ScriptSettings {
	pub fn remap_entities(&mut self, remap: &HashMap<EntityKey, EntityKey>) {
		for reference in self.references.values_mut() {
			if let ScriptReference::Entity { entity } = reference
				&& let Some(mapped) = remap.get(entity)
			{
				entity.clone_from(mapped);
			}
		}
	}

	pub(crate) fn bind_slots(
		&mut self,
		bindings: &BTreeMap<String, EntityKey>,
	) -> Result<(), String> {
		for reference in self.references.values_mut() {
			if let ScriptReference::Slot { slot } = reference {
				let entity = bindings
					.get(slot)
					.ok_or_else(|| format!("Script reference slot '{slot}' is unbound"))?;
				*reference = ScriptReference::Entity {
					entity: entity.clone(),
				};
			}
		}
		Ok(())
	}

	pub fn validate(&self) -> Result<(), String> {
		if self.script_id.is_empty()
			|| self.script_id.len() > 128
			|| self.script_id.split('.').any(|part| !valid_field(part))
		{
			return Err("Invalid script ID".into());
		}
		if self.config_version == 0
			|| self.properties.len() > MAX_ENTRIES
			|| self.references.len() > MAX_ENTRIES
			|| self.properties.keys().any(|name| !valid_field(name))
			|| self.references.keys().any(|name| !valid_field(name))
		{
			return Err("Invalid script configuration version or field names".into());
		}
		for property in self.properties.values() {
			match property {
				ScriptProperty::Number(value) if !value.is_finite() => {
					return Err("Script properties must be finite".into());
				}
				ScriptProperty::Text(value) if value.len() > MAX_PROPERTY_TEXT_BYTES => {
					return Err("Script text property exceeds its byte limit".into());
				}
				_ => {}
			}
		}
		if self.references.values().any(
			|reference| matches!(reference, ScriptReference::Slot { slot } if !valid_field(slot)),
		) {
			return Err("Invalid script reference slot".into());
		}
		Ok(())
	}

	pub(crate) fn retained_bytes(&self) -> usize {
		self.script_id.capacity()
			+ self
				.properties
				.iter()
				.map(|(name, value)| {
					name.capacity()
						+ size_of::<ScriptProperty>()
						+ match value {
							ScriptProperty::Text(text) => text.capacity(),
							_ => 0,
						}
				})
				.sum::<usize>()
			+ self
				.references
				.iter()
				.map(|(name, value)| {
					name.capacity()
						+ size_of::<ScriptReference>()
						+ match value {
							ScriptReference::Entity { entity } => entity.as_str().len(),
							ScriptReference::Slot { slot } => slot.capacity(),
						}
				})
				.sum::<usize>()
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptComponent {
	pub id: ComponentId,
	pub enabled: bool,
	pub settings: ScriptSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScriptField {
	Number {
		key: String,
		label: String,
		default: f32,
		min: f32,
		max: f32,
	},
	Boolean {
		key: String,
		label: String,
		default: bool,
	},
	Text {
		key: String,
		label: String,
		default: String,
		max_bytes: usize,
	},
}

impl ScriptField {
	pub fn key(&self) -> &str {
		match self {
			Self::Number { key, .. } | Self::Boolean { key, .. } | Self::Text { key, .. } => key,
		}
	}

	fn accepts(&self, value: &ScriptProperty) -> bool {
		match (self, value) {
			(Self::Number { min, max, .. }, ScriptProperty::Number(value)) => {
				(*min..=*max).contains(value)
			}
			(Self::Boolean { .. }, ScriptProperty::Boolean(_)) => true,
			(Self::Text { max_bytes, .. }, ScriptProperty::Text(value)) => {
				value.len() <= *max_bytes
			}
			_ => false,
		}
	}

	pub fn default_value(&self) -> ScriptProperty {
		match self {
			Self::Number { default, .. } => ScriptProperty::Number(*default),
			Self::Boolean { default, .. } => ScriptProperty::Boolean(*default),
			Self::Text { default, .. } => ScriptProperty::Text(default.clone()),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptReferenceKind {
	Entity,
	TriggerCollider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptReferenceField {
	pub key: String,
	pub label: String,
	pub requires: ScriptReferenceKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptDescriptor {
	pub id: String,
	pub label: String,
	pub config_version: u32,
	pub fields: Vec<ScriptField>,
	pub references: Vec<ScriptReferenceField>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub receive_local_actions: bool,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub write_owner_transform: bool,
}

#[derive(Debug)]
pub enum ScriptIssue {
	MissingRegistration(String),
	InvalidConfiguration(String),
	InvalidReference {
		field: String,
		target: Option<EntityKey>,
		reason: String,
	},
}

impl ScriptIssue {
	pub fn code(&self) -> &'static str {
		match self {
			Self::MissingRegistration(_) => "missing_script",
			Self::InvalidConfiguration(_) => "script_configuration_invalid",
			Self::InvalidReference { .. } => "script_reference_invalid",
		}
	}

	pub fn message(&self) -> String {
		match self {
			Self::MissingRegistration(id) => format!("Unknown script {id}"),
			Self::InvalidConfiguration(reason) | Self::InvalidReference { reason, .. } => {
				reason.clone()
			}
		}
	}

	pub fn at_attachment(&self, owner: &EntityKey, component: &ComponentId) -> String {
		format!(
			"Object {} Script {} [{}]: {}",
			owner.as_str(),
			component.as_str(),
			self.code(),
			self.message(),
		)
	}
}

impl ScriptDescriptor {
	/// External reference capabilities can be deferred when checking a reusable Prop.
	pub fn validate_attachment<'a>(
		&self,
		owner: &EntityData,
		settings: &ScriptSettings,
		find_entity: impl Fn(&EntityKey) -> Option<&'a EntityData>,
		is_external: impl Fn(&ScriptReference) -> bool,
	) -> Result<(), ScriptIssue> {
		self.validate_config(settings)
			.map_err(ScriptIssue::InvalidConfiguration)?;
		if self.write_owner_transform
			&& owner
				.character_controller
				.as_ref()
				.is_some_and(|component| component.enabled)
		{
			return Err(ScriptIssue::InvalidConfiguration(format!(
				"{} writes its owner's transform, which is owned by the enabled Character Controller",
				self.label
			)));
		}
		for field in &self.references {
			let invalid = |target, reason| ScriptIssue::InvalidReference {
				field: field.key.clone(),
				target,
				reason,
			};
			let reference = settings.references.get(&field.key).ok_or_else(|| {
				invalid(
					None,
					format!("{} requires the {} reference", self.label, field.label),
				)
			})?;
			if is_external(reference) {
				continue;
			}
			let ScriptReference::Entity { entity } = reference else {
				return Err(invalid(
					None,
					format!("Bind the {} slot before Play", field.label),
				));
			};
			let target = find_entity(entity).ok_or_else(|| {
				invalid(
					Some(entity.clone()),
					format!("Missing {} entity {entity:?}", field.label),
				)
			})?;
			if field.requires == ScriptReferenceKind::TriggerCollider
				&& !(target.collision_enabled
					&& target
						.collider
						.as_ref()
						.is_some_and(|collider| collider.interaction().sensor))
			{
				return Err(invalid(
					Some(entity.clone()),
					format!("{} must have an enabled trigger collider", field.label),
				));
			}
		}
		Ok(())
	}

	fn validate_config(&self, settings: &ScriptSettings) -> Result<(), String> {
		settings.validate()?;
		if settings.script_id != self.id || settings.config_version != self.config_version {
			return Err(format!(
				"{} has an unsupported script configuration version",
				self.label
			));
		}
		for (key, value) in &settings.properties {
			if !self
				.fields
				.iter()
				.any(|field| field.key() == key && field.accepts(value))
			{
				return Err(format!("{} has an invalid property {key}", self.label));
			}
		}
		for key in settings.references.keys() {
			if !self.references.iter().any(|field| field.key == *key) {
				return Err(format!("{} has an unknown reference {key}", self.label));
			}
		}
		Ok(())
	}

	pub fn property(&self, settings: &ScriptSettings, key: &str) -> Option<ScriptProperty> {
		settings.properties.get(key).cloned().or_else(|| {
			self.fields
				.iter()
				.find(|field| field.key() == key)
				.map(ScriptField::default_value)
		})
	}
}

fn valid_field(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 64
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
