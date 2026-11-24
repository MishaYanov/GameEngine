//! Evidence supplied by a validated glTF model for persistent mesh identities.
use std::{
	collections::{HashMap, HashSet},
	path::Path,
};

use engine_core::asset_catalog::{MeshIdentity, MeshSignature, MeshSubassetId, PersistentAsset};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{MAX_SOURCE, asset_path, fingerprint, read};
use crate::assets::gltf_loader::{self, ImportedPrimitive};

#[derive(Deserialize)]
struct MeshDocument {
	#[serde(default)]
	meshes: Vec<MeshExtras>,
}

#[derive(Deserialize)]
struct MeshExtras {
	#[serde(default)]
	extras: serde_json::Value,
}

pub(super) fn signatures(
	path: &Path,
	asset_root: &Path,
	self_contained: bool,
) -> Result<Vec<MeshSignature>, String> {
	let bytes = read(path, MAX_SOURCE)?;
	if self_contained && !bytes.starts_with(b"glTF") {
		return Err("External imports require a binary .glb file".into());
	}
	let mut budget = crate::assets::import_budget::ImportBudget::new(None);
	let binary = bytes
		.starts_with(b"glTF")
		.then(|| gltf::binary::Glb::from_slice(&bytes))
		.transpose()
		.map_err(|error| error.to_string())?;
	let json = binary
		.as_ref()
		.map_or(bytes.as_slice(), |binary| binary.json.as_ref());
	budget
		.preflight_json(json)
		.map_err(|error| error.to_string())?;
	let source_keys = source_keys(json)?;
	let parsed = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
	if parsed.skins().len() > 0 {
		// The shared CPU decoder owns skin/clip validation; the static scene
		// importer deliberately rejects skins and is not used for this path.
		bridge::animation_asset::decode(&bytes)?;
	}
	if self_contained
		&& (parsed.buffers().any(
			|b| matches!(b.source(),gltf::buffer::Source::Uri(uri) if !uri.starts_with("data:")),
		) || parsed.images().any(
			|i| matches!(i.source(),gltf::image::Source::Uri{uri,..} if !uri.starts_with("data:")),
		)) {
		return Err("External GLB imports must embed their buffers and images".into());
	}
	let mesh_count = parsed.meshes().len();
	if mesh_count == 0 || mesh_count > 10_000 || source_keys.len() != mesh_count {
		return Err("Model requires 1–10000 meshes with matching metadata".into());
	}
	gltf_loader::preflight_meshes(&parsed.document, parsed.meshes().map(|mesh| mesh.index()))
		.map_err(|error| error.to_string())?;
	// Validate all images and geometry before the candidate enters the catalog.
	let mut resources =
		gltf_loader::load_resources_in_root(path, asset_root).map_err(|error| error.to_string())?;
	resources
		.prepare_all_images()
		.map_err(|error| error.to_string())?;
	let mut meshes = Vec::with_capacity(mesh_count);
	for (mesh, source_key) in parsed.meshes().zip(source_keys) {
		let parts = resources
			.import_mesh(mesh.index())
			.map_err(|error| error.to_string())?;
		meshes.push(MeshSignature {
			index: mesh.index(),
			name: mesh.name().unwrap_or("Mesh").to_owned(),
			primitives: mesh.primitives().len(),
			identity: Some(MeshIdentity {
				id: MeshSubassetId::generate(),
				geometry_sha256: geometry_digest(&parts),
				source_key,
			}),
		});
	}
	Ok(meshes)
}

/// Populate missing evidence from the existing, still-fingerprinted source
/// without writing it or altering legacy path/index references.
pub(super) fn hydrate_legacy_meshes(
	asset_root: &Path,
	asset: &mut PersistentAsset,
) -> Result<(), String> {
	if asset.meshes.is_empty() || asset.meshes.iter().all(|mesh| mesh.identity.is_some()) {
		return Ok(());
	}
	let path = asset_path(asset_root, &asset.path, true)?;
	let bytes = read(&path, MAX_SOURCE)?;
	if fingerprint(&bytes) != asset.fingerprint {
		return Err(format!(
			"Cannot establish mesh identities for {} because its source changed outside the catalog",
			asset.path
		));
	}
	let inspected = signatures(&path, asset_root, false)?
		.into_iter()
		.map(|mesh| (mesh.index, mesh))
		.collect::<HashMap<_, _>>();
	if read(&path, MAX_SOURCE)? != bytes {
		return Err(format!(
			"Model {} changed while its mesh identities were inspected",
			asset.path
		));
	}
	for mesh in &mut asset.meshes {
		if mesh.identity.is_some() {
			continue;
		}
		let original = inspected
			.get(&mesh.index)
			.filter(|original| original.name == mesh.name && original.primitives == mesh.primitives)
			.ok_or_else(|| {
				format!(
					"Catalog mesh {} in {} no longer matches its source",
					mesh.index, asset.path
				)
			})?;
		mesh.identity.clone_from(&original.identity);
	}
	asset.validate()
}

fn source_keys(json: &[u8]) -> Result<Vec<Option<String>>, String> {
	let document: MeshDocument = serde_json::from_slice(json).map_err(|error| error.to_string())?;
	let mut keys = HashSet::new();
	document
		.meshes
		.into_iter()
		.enumerate()
		.map(|(index, mesh)| {
			let Some(key) = mesh.extras.get("game_engine_mesh_id") else {
				return Ok(None);
			};
			let key = key
				.as_str()
				.filter(|key| {
					!key.is_empty()
						&& key.len() <= 128
						&& key
							.bytes()
							.all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
				})
				.ok_or_else(|| format!("Mesh {index} has an invalid extras.game_engine_mesh_id"))?;
			if !keys.insert(key.to_owned()) {
				return Err(format!(
					"Mesh {index} duplicates extras.game_engine_mesh_id={key}"
				));
			}
			Ok(Some(key.to_owned()))
		})
		.collect()
}

fn geometry_digest(parts: &[ImportedPrimitive]) -> String {
	let mut hash = Sha256::new();
	hash.update(b"engine-mesh-geometry-v1");
	hash.update((parts.len() as u64).to_le_bytes());
	for part in parts {
		let vertices = part.mesh.vertices();
		let indices = part.mesh.indices();
		hash.update((vertices.len() as u64).to_le_bytes());
		for vertex in vertices {
			for value in vertex
				.position
				.into_iter()
				.chain(vertex.uv)
				.chain(vertex.normal)
			{
				hash.update(value.to_le_bytes());
			}
		}
		hash.update((indices.len() as u64).to_le_bytes());
		for &index in indices {
			hash.update(index.to_le_bytes());
		}
	}
	format!("{:x}", hash.finalize())
}
