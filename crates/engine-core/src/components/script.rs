//! Saved, bounded settings and reference bindings for a compiled object script.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::EntityKey;

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

impl ScriptDescriptor {
	pub fn validate_settings(&self, settings: &ScriptSettings) -> Result<(), String> {
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
		for field in &self.references {
			if !settings.references.contains_key(&field.key) {
				return Err(format!(
					"{} requires the {} reference",
					self.label, field.label
				));
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
