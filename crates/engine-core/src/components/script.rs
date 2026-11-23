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

fn valid_field(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 64
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
