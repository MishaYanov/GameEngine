use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityKey(String);

impl EntityKey {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		let key = Self(value.into());
		key.validate()?;
		Ok(key)
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}

	pub(crate) fn validate(&self) -> Result<(), String> {
		if self.0.is_empty() || self.0.chars().any(char::is_whitespace) {
			return Err("Entity key must be nonempty and contain no whitespace".to_string());
		}
		Ok(())
	}

	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}
}

#[cfg(test)]
#[path = "tests_entity_key.rs"]
mod tests;
