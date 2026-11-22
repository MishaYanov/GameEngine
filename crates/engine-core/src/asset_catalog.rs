//! Persistent authored asset identity. Filesystem policy belongs to the adapters.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

use crate::scene_data::validate_model_path;

pub const CATALOG_FILE: &str = "asset-catalog.json";
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CATALOG_ASSETS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AssetId(String);

impl AssetId {
	pub fn new(value: impl Into<String>) -> Result<Self, String> {
		let value = value.into();
		if value.len() != 36 {
			return Err("Asset identity must be a canonical UUID".into());
		}
		let parsed = uuid::Uuid::parse_str(&value).map_err(|_| "Invalid asset UUID")?;
		if parsed.hyphenated().to_string() != value || parsed.is_nil() {
			return Err("Asset identity must be a nonempty lowercase canonical UUID".into());
		}
		Ok(Self(value))
	}
	pub fn generate() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for AssetId {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
	AnimatorGraph,
	InputActions,
	Model,
	Prefab,
	Scene,
	Texture,
	AnimationClip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSignature {
	pub index: usize,
	pub name: String,
	pub primitives: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistentAsset {
	pub id: AssetId,
	pub path: String,
	pub kind: AssetKind,
	/// Former logical paths remain reserved so legacy references cannot retarget.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub aliases: Vec<String>,
	pub revision: u64,
	pub fingerprint: String,
	pub bytes: u64,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_path: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub meshes: Vec<MeshSignature>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub dependencies: Vec<AssetId>,
}

impl PersistentAsset {
	pub fn validate(&self) -> Result<(), String> {
		validate_model_path(&self.path)?;
		if self.kind == AssetKind::AnimatorGraph
			&& (!self
				.path
				.to_ascii_lowercase()
				.ends_with(crate::animation::ANIMATOR_GRAPH_SUFFIX)
				|| self.bytes > crate::animation::MAX_ANIMATOR_GRAPH_BYTES as u64)
		{
			return Err(
				"Animator catalog entry requires a .animator.json asset within 256 KiB".into(),
			);
		}
		if self.kind == AssetKind::InputActions
			&& (!self
				.path
				.to_ascii_lowercase()
				.ends_with(crate::input::INPUT_ACTION_MAP_SUFFIX)
				|| self.bytes > crate::input::MAX_INPUT_ACTION_MAP_BYTES as u64)
		{
			return Err(
				"Input Actions catalog entry requires a .input-actions.json asset within 64 KiB"
					.into(),
			);
		}
		if self.revision == 0
			|| self.fingerprint.len() != 64
			|| !self
				.fingerprint
				.bytes()
				.all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
			|| self.bytes > 64 * 1024 * 1024
		{
			return Err("Asset revision, fingerprint or source byte budget is invalid".into());
		}
		if self.aliases.len() > 64
			|| self.meshes.len() > 10_000
			|| self.dependencies.len() > MAX_CATALOG_ASSETS
		{
			return Err("Asset metadata exceeds its bounded reference limits".into());
		}
		for alias in &self.aliases {
			validate_model_path(alias)?;
		}
		if self
			.source_path
			.as_ref()
			.is_some_and(|path| path.len() > 4096 || path.chars().any(char::is_control))
		{
			return Err("Asset source path is oversized or contains control characters".into());
		}
		let mut indices = HashSet::new();
		for mesh in &self.meshes {
			if mesh.index > 1_000_000
				|| mesh.name.len() > 256
				|| mesh.name.chars().any(char::is_control)
				|| mesh.primitives == 0
				|| mesh.primitives > 10_000
				|| !indices.insert(mesh.index)
			{
				return Err("Asset mesh signature is invalid or duplicated".into());
			}
		}
		if self.kind == AssetKind::AnimationClip
			&& (!self.path.to_ascii_lowercase().ends_with(".clip.json")
				|| self.bytes > crate::animation::MAX_AUTHORED_CLIP_BYTES as u64)
		{
			return Err(
				"Animation Clip catalog entries require a .clip.json path and at most 1 MiB".into(),
			);
		}
		if self.kind != AssetKind::Model && !self.meshes.is_empty() {
			return Err("Only model assets can declare meshes".into());
		}
		Ok(())
	}
	pub fn check_reimport(
		&self,
		replacement: &Self,
		used_meshes: &HashSet<usize>,
	) -> Result<(), String> {
		replacement.validate()?;
		if self.id != replacement.id
			|| self.kind != replacement.kind
			|| self.path != replacement.path
		{
			return Err("Reimport must preserve asset identity, kind and destination".into());
		}
		for index in used_meshes {
			let old = self
				.meshes
				.iter()
				.find(|mesh| mesh.index == *index)
				.ok_or("Referenced original mesh is unavailable")?;
			let next = replacement
				.meshes
				.iter()
				.find(|mesh| mesh.index == *index)
				.ok_or_else(|| format!("Reimport removed referenced mesh {index}"))?;
			if old.name != next.name || old.primitives != next.primitives {
				return Err(format!(
					"Reimport changed the identity/layout of referenced mesh {index}"
				));
			}
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetCatalog {
	pub schema_version: u32,
	pub assets: Vec<PersistentAsset>,
}

impl Default for AssetCatalog {
	fn default() -> Self {
		Self {
			schema_version: 1,
			assets: Vec::new(),
		}
	}
}

impl AssetCatalog {
	pub fn validate(&self) -> Result<(), String> {
		if self.schema_version != 1 || self.assets.len() > MAX_CATALOG_ASSETS {
			return Err("Asset catalog requires version 1 and at most 4096 records".into());
		}
		let mut identities = HashSet::new();
		let mut paths = HashMap::new();
		let mut source_bytes = 0u64;
		for asset in &self.assets {
			asset.validate()?;
			source_bytes = source_bytes
				.checked_add(asset.bytes)
				.filter(|bytes| *bytes <= 512 * 1024 * 1024)
				.ok_or("Asset catalog source budget exceeds 512 MiB")?;
			if !identities.insert(&asset.id) {
				return Err("Duplicate persistent asset identity".into());
			}
			for path in std::iter::once(&asset.path).chain(&asset.aliases) {
				if paths.insert(path.to_lowercase(), &asset.id).is_some() {
					return Err(format!("Asset path/alias is already reserved: {path}"));
				}
			}
		}
		for asset in &self.assets {
			if asset
				.dependencies
				.iter()
				.any(|dependency| !identities.contains(dependency))
			{
				return Err("Asset dependency has an unresolved persistent identity".into());
			}
		}
		Ok(())
	}
	pub fn from_json(source: &str) -> Result<Self, String> {
		if source.len() > MAX_CATALOG_BYTES {
			return Err("Asset catalog exceeds 4 MiB".into());
		}
		let value: serde_json::Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
		if !value.is_object()
			|| value
				.get("assets")
				.and_then(|v| v.as_array())
				.is_none_or(|assets| assets.iter().any(|asset| !asset.is_object()))
		{
			return Err("Asset catalog and records must be objects".into());
		}
		let catalog: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
		catalog.validate()?;
		Ok(catalog)
	}
	pub fn to_json(&self) -> Result<String, String> {
		self.validate()?;
		let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
		if json.len() > MAX_CATALOG_BYTES {
			return Err("Asset catalog exceeds 4 MiB".into());
		}
		Ok(json)
	}
	pub fn by_id(&self, id: &AssetId) -> Option<&PersistentAsset> {
		self.assets.iter().find(|asset| &asset.id == id)
	}
	pub fn by_path(&self, path: &str) -> Option<&PersistentAsset> {
		let folded = path.to_lowercase();
		self.assets.iter().find(|asset| {
			asset.path.to_lowercase() == folded
				|| asset
					.aliases
					.iter()
					.any(|alias| alias.to_lowercase() == folded)
		})
	}
	pub fn resolve(
		&self,
		id: Option<&AssetId>,
		legacy_path: &str,
		mesh_index: Option<usize>,
	) -> Result<Option<&PersistentAsset>, String> {
		let asset = match id {
			Some(id) => Some(
				self.by_id(id)
					.ok_or_else(|| format!("Persistent asset {} is unavailable", id.as_str()))?,
			),
			None => self.by_path(legacy_path),
		};
		if let Some(asset) = asset
			&& asset.kind == AssetKind::Model
			&& !asset
				.meshes
				.iter()
				.any(|mesh| mesh.index == mesh_index.unwrap_or(0))
		{
			return Err(format!(
				"Asset {} has no compatible mesh {}",
				asset.id.as_str(),
				mesh_index.unwrap_or(0)
			));
		}
		Ok(asset)
	}
	pub fn replace(&mut self, record: PersistentAsset) -> Result<(), String> {
		let mut candidate = self.clone();
		if let Some(existing) = candidate
			.assets
			.iter_mut()
			.find(|asset| asset.id == record.id)
		{
			*existing = record;
		} else {
			candidate.assets.push(record);
		}
		candidate.to_json()?;
		*self = candidate;
		Ok(())
	}
}
