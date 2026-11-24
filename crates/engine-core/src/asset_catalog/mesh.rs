//! Stable model mesh identities and evidence for reimport reconciliation.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{AssetId, PersistentAsset};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MeshSubassetId(AssetId);

impl MeshSubassetId {
	pub fn generate() -> Self {
		Self(AssetId::generate())
	}

	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshIdentity {
	pub id: MeshSubassetId,
	/// SHA-256 of decoded mesh geometry in the geometry-v1 encoding.
	pub geometry_sha256: String,
	/// Optional identity declared by the source exporter, unique in this model.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub source_key: Option<String>,
}

impl MeshIdentity {
	pub(super) fn validate(&self) -> Result<(), String> {
		if self.geometry_sha256.len() != 64
			|| !self
				.geometry_sha256
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err("Mesh identity requires a lowercase SHA-256 geometry digest".into());
		}
		if self.source_key.as_ref().is_some_and(|key| {
			key.is_empty()
				|| key.len() > 128
				|| !key
					.bytes()
					.all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
		}) {
			return Err("Mesh source key requires 1–128 ASCII letters, digits, periods, underscores, or hyphens".into());
		}
		Ok(())
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSignature {
	pub index: usize,
	pub name: String,
	pub primitives: usize,
	/// Legacy catalogs omit this field and keep using their numeric mesh slots.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub identity: Option<MeshIdentity>,
}

impl MeshSignature {
	fn source_key(&self) -> Option<&str> {
		self.identity.as_ref()?.source_key.as_deref()
	}
}

pub(super) fn reimport_mesh_matches<'a>(
	previous: &'a PersistentAsset,
	replacement: &PersistentAsset,
) -> Vec<Option<&'a MeshSignature>> {
	let old_meshes = &previous.meshes;
	let new_meshes = &replacement.meshes;
	let by_index = old_meshes
		.iter()
		.map(|mesh| (mesh.index, mesh))
		.collect::<HashMap<_, _>>();
	let by_source_key = old_meshes
		.iter()
		.filter_map(|mesh| Some((mesh.source_key()?, mesh)))
		.collect::<HashMap<_, _>>();
	let old_geometry = unique_geometry(old_meshes);
	let new_geometry = unique_geometry(new_meshes);
	let unchanged_source = previous.fingerprint == replacement.fingerprint;
	let single_mesh = old_meshes.len() == 1 && new_meshes.len() == 1;
	new_meshes
		.iter()
		.map(|candidate| {
			let same_slot = by_index.get(&candidate.index).copied();
			if unchanged_source {
				return same_slot.filter(|old| {
					old.name == candidate.name && old.primitives == candidate.primitives
				});
			}
			if let Some(old) = candidate
				.source_key()
				.and_then(|key| by_source_key.get(key))
			{
				return Some(*old);
			}
			if single_mesh
				&& let Some(old) = same_slot
				&& old.source_key().is_none()
				&& candidate.source_key().is_none()
				&& old.name == candidate.name
				&& old.primitives == candidate.primitives
			{
				// A whole-asset replacement has no competing mesh in either revision.
				return Some(old);
			}
			let digest = candidate.identity.as_ref()?.geometry_sha256.as_str();
			new_geometry.get(digest).copied().flatten()?;
			old_geometry
				.get(digest)
				.copied()
				.flatten()
				.filter(|old| old.source_key().is_none())
		})
		.collect()
}

fn unique_geometry(meshes: &[MeshSignature]) -> HashMap<&str, Option<&MeshSignature>> {
	let mut geometry = HashMap::new();
	for mesh in meshes {
		if let Some(identity) = &mesh.identity {
			geometry
				.entry(identity.geometry_sha256.as_str())
				.and_modify(|candidate| *candidate = None)
				.or_insert(Some(mesh));
		}
	}
	geometry
}
