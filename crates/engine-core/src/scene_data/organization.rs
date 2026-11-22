//! Authored Hierarchy folders and their scene-root membership.

use super::EntityData;
use crate::EntityKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_SCENE_FOLDERS: usize = 1024;
pub const MAX_FOLDER_DEPTH: usize = 32;
pub const MAX_FOLDER_NAME_BYTES: usize = 128;
pub const MAX_FOLDER_ID_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct FolderId(String);

impl FolderId {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		Self::try_from(value.into())
	}

	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<String> for FolderId {
	type Error = String;

	fn try_from(value: String) -> Result<Self, Self::Error> {
		if value.is_empty()
			|| value.len() > MAX_FOLDER_ID_BYTES
			|| !value
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err(format!(
				"Folder ID requires 1–{MAX_FOLDER_ID_BYTES} ASCII letters, digits, underscores or hyphens"
			));
		}
		Ok(Self(value))
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderRecord {
	pub id: FolderId,
	pub name: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub parent: Option<FolderId>,
}

impl FolderRecord {
	pub fn validate_name(name: &str) -> Result<(), String> {
		if name.is_empty()
			|| name.len() > MAX_FOLDER_NAME_BYTES
			|| name.trim() != name
			|| name.chars().any(char::is_control)
		{
			return Err(format!(
				"Folder name must be nonblank, trimmed, and control-free with at most {MAX_FOLDER_NAME_BYTES} UTF-8 bytes"
			));
		}
		Ok(())
	}
}

/// Scene-local organization for the editor Hierarchy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SceneOrganization {
	/// The order of records under the same parent is their display order.
	pub folders: Vec<FolderRecord>,
	/// Each scene-root entity can belong to one folder. Descendants follow their transform parent.
	pub root_membership: BTreeMap<EntityKey, FolderId>,
}

impl SceneOrganization {
	pub fn is_empty(&self) -> bool {
		self.folders.is_empty() && self.root_membership.is_empty()
	}

	pub fn validate(&self, entities: &[EntityData]) -> Result<(), String> {
		if self.is_empty() {
			return Ok(());
		}
		if self.folders.len() > MAX_SCENE_FOLDERS {
			return Err(format!(
				"A scene supports at most {MAX_SCENE_FOLDERS} folders"
			));
		}
		let mut by_id = HashMap::with_capacity(self.folders.len());
		for folder in &self.folders {
			let id = folder.id.as_str();
			FolderRecord::validate_name(&folder.name)
				.map_err(|error| format!("Folder {id:?}: {error}"))?;
			if by_id.insert(&folder.id, folder).is_some() {
				return Err(format!("Duplicate folder ID {id:?}"));
			}
		}
		let mut path = Vec::with_capacity(MAX_FOLDER_DEPTH);
		for folder in &self.folders {
			path.clear();
			let mut current = Some(folder);
			while let Some(ancestor) = current {
				if path.contains(&&ancestor.id) {
					return Err(format!("Folder parent cycle at {:?}", ancestor.id.as_str()));
				}
				path.push(&ancestor.id);
				if path.len() > MAX_FOLDER_DEPTH {
					return Err(format!("Folder nesting exceeds depth {MAX_FOLDER_DEPTH}"));
				}
				current = match &ancestor.parent {
					Some(parent) => Some(*by_id.get(parent).ok_or_else(|| {
						format!(
							"Folder {:?} references missing parent folder {:?}",
							ancestor.id.as_str(),
							parent.as_str()
						)
					})?),
					None => None,
				};
			}
		}
		if !self.root_membership.is_empty() {
			let roots: HashSet<_> = entities
				.iter()
				.filter(|entity| entity.parent.is_none())
				.map(|entity| &entity.key)
				.collect();
			for (entity, folder) in &self.root_membership {
				if !roots.contains(entity) {
					return Err(format!(
						"Folder membership requires an existing scene-root object, got {:?}",
						entity.as_str()
					));
				}
				if !by_id.contains_key(folder) {
					return Err(format!(
						"Membership references missing folder {:?}",
						folder.as_str()
					));
				}
			}
		}
		Ok(())
	}
}
