use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::HashMap, fmt};

pub const MAX_PROJECT_SCENES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneDescriptorErrors {
	SceneNotFound,
	DuplicateSceneId,
	DuplicateScenePath,
	InvalidScenePath(String),
}

impl fmt::Display for SceneDescriptorErrors {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::SceneNotFound => write!(f, "Scene is not registered in the catalog"),
			Self::DuplicateSceneId => write!(f, "Scene ID already exists in the catalog"),
			Self::DuplicateScenePath => write!(f, "Scene path already exists in the catalog"),
			Self::InvalidScenePath(message) => write!(f, "{message}"),
		}
	}
}

impl std::error::Error for SceneDescriptorErrors {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SceneId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDescriptor {
	id: SceneId,
	asset_path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneCatalog {
	scenes: HashMap<SceneId, SceneDescriptor>,
}

impl SceneId {
	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}

	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		let value = value.into();

		if value.is_empty()
			|| value.len() > 256
			|| value.chars().any(|c| c.is_whitespace() || c.is_control())
		{
			return Err(
				"Scene ID must be nonempty and contain no whitespace or control characters".into(),
			);
		}

		Ok(Self(value))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl SceneDescriptor {
	pub fn new(id: SceneId, asset_path: impl Into<String>) -> Result<Self, String> {
		let asset_path = asset_path.into();

		crate::scene_data::validate_model_path(&asset_path)
			.map_err(|error| format!("Scene path: {error}"))?;

		if !asset_path.to_ascii_lowercase().ends_with(".json") {
			return Err("Scene path must name a JSON scene".into());
		}

		Ok(Self { id, asset_path })
	}

	pub fn id(&self) -> &SceneId {
		&self.id
	}

	pub fn asset_path(&self) -> &str {
		&self.asset_path
	}

	pub fn set_asset_path(&mut self, asset_path: impl Into<String>) -> Result<(), String> {
		let replacement = Self::new(self.id.clone(), asset_path)?;

		self.asset_path = replacement.asset_path;
		Ok(())
	}
}

impl SceneCatalog {
	pub fn get(&self, id: &SceneId) -> Result<&SceneDescriptor, SceneDescriptorErrors> {
		self.scenes
			.get(id)
			.ok_or(SceneDescriptorErrors::SceneNotFound)
	}

	pub fn iter(&self) -> impl Iterator<Item=&SceneDescriptor> {
		self.scenes.values()
	}

	pub fn insert(&mut self, descriptor: SceneDescriptor) -> Result<(), SceneDescriptorErrors> {
		if self.scenes.contains_key(descriptor.id()) {
			return Err(SceneDescriptorErrors::DuplicateSceneId);
		}
		if self.scenes.len() >= MAX_PROJECT_SCENES {
			return Err(SceneDescriptorErrors::InvalidScenePath(
				"Project scene limit exceeded".into(),
			));
		}
		if self
			.scenes
			.values()
			.any(|scene| scene.asset_path() == descriptor.asset_path())
		{
			return Err(SceneDescriptorErrors::DuplicateScenePath);
		}

		self.scenes.insert(descriptor.id().clone(), descriptor);
		Ok(())
	}

	pub fn change_path(
		&mut self,
		id: &SceneId,
		new_path: impl Into<String>,
	) -> Result<(), SceneDescriptorErrors> {
		self.get(id)?;
		let new_path = new_path.into();
		if self
			.scenes
			.values()
			.any(|scene| scene.id() != id && scene.asset_path() == new_path)
		{
			return Err(SceneDescriptorErrors::DuplicateScenePath);
		}

		self.scenes
			.get_mut(id)
			.ok_or(SceneDescriptorErrors::SceneNotFound)?
			.set_asset_path(new_path)
			.map_err(SceneDescriptorErrors::InvalidScenePath)
	}

	pub fn remove(&mut self, id: &SceneId) -> Result<(), SceneDescriptorErrors> {
		self.scenes
			.remove(id)
			.map(|_| ())
			.ok_or(SceneDescriptorErrors::SceneNotFound)
	}
}

impl Serialize for SceneId {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(self.as_str())
	}
}
impl<'de> Deserialize<'de> for SceneId {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}
impl Serialize for SceneDescriptor {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		use serde::ser::SerializeStruct;
		let mut value = serializer.serialize_struct("SceneDescriptor", 2)?;
		value.serialize_field("id", &self.id)?;
		value.serialize_field("asset_path", &self.asset_path)?;
		value.end()
	}
}
impl<'de> Deserialize<'de> for SceneDescriptor {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct Entry {
			id: SceneId,
			asset_path: String,
		}
		let value = Entry::deserialize(deserializer)?;
		Self::new(value.id, value.asset_path).map_err(serde::de::Error::custom)
	}
}
impl Serialize for SceneCatalog {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		let mut entries: Vec<_> = self.iter().collect();
		entries.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));
		entries.serialize(serializer)
	}
}
impl<'de> Deserialize<'de> for SceneCatalog {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		let entries = Vec::<SceneDescriptor>::deserialize(deserializer)?;
		let mut catalog = Self::default();
		for entry in entries {
			catalog.insert(entry).map_err(serde::de::Error::custom)?;
		}
		Ok(catalog)
	}
}

#[cfg(test)]
#[path = "tests_scene_catalog.rs"]
mod tests;
