//! Shared bounded GLB animation decoding for editor, native player, compiler and headless.
use crate::asset_io::{contained_file, read_bounded};
use engine_core::{
	SceneData,
	animation::{
		AnimationAsset, AnimationSourceRef, MAX_ANIMATION_BYTES, MAX_ANIMATION_INSTANCES,
		PreparedAnimationBindings,
	},
	asset_catalog::{AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES},
};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, sync::Arc};

mod clips;
mod decode;
mod identity;
mod mesh;
mod rig;

pub use decode::decode;

#[derive(Clone, Debug, Serialize)]
pub struct AnimationClipMetadata {
	pub index: u32,
	pub name: String,
	pub duration_seconds: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimationJointMetadata {
	pub source_node: u32,
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub stable_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub parent_source_node: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnimationMetadata {
	pub source: AnimationSourceRef,
	pub mesh_index: u32,
	pub nodes: usize,
	pub joints: usize,
	pub joint_metadata: Vec<AnimationJointMetadata>,
	pub clips: Vec<AnimationClipMetadata>,
}
fn root_and_catalog(root: &Path) -> Result<(std::path::PathBuf, AssetCatalog), String> {
	let meta = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
	if !meta.is_dir() || meta.file_type().is_symlink() {
		return Err("Animation root must be an ordinary directory".into());
	}
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let bytes = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
	let catalog = AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
	Ok((root, catalog))
}
pub fn inspect(root: &Path, id: &AssetId) -> Result<AnimationMetadata, String> {
	let (root, catalog) = root_and_catalog(root)?;
	let entry = catalog
		.by_id(id)
		.ok_or("Animation asset ID is missing from catalog")?;
	if entry.kind != AssetKind::Model {
		return Err("Animation requires a model asset".into());
	}
	let asset = load_path(&root, &entry.path)?;
	if asset.character.is_some() && asset.source_sha256 != entry.fingerprint {
		return Err(
			"Character source changed outside its catalog; reimport the asset first".into(),
		);
	}
	Ok(AnimationMetadata {
		source: asset.source_ref(id.clone()),
		mesh_index: asset.skin.mesh_index,
		nodes: asset.nodes.len(),
		joints: asset.skin.joints.len(),
		joint_metadata: asset
			.skin
			.joints
			.iter()
			.map(|&index| {
				let node = &asset.nodes[usize::from(index)];
				AnimationJointMetadata {
					source_node: node.source_node,
					name: node
						.name
						.clone()
						.unwrap_or_else(|| format!("Joint {}", node.source_node)),
					stable_id: asset
						.character
						.as_ref()
						.and_then(|character| character.joints.get(&node.source_node))
						.cloned(),
					parent_source_node: node
						.parent
						.map(|parent| asset.nodes[usize::from(parent)].source_node),
				}
			})
			.collect(),
		clips: asset
			.clips
			.iter()
			.map(|c| AnimationClipMetadata {
				index: c.source_index,
				name: c.name.clone(),
				duration_seconds: c.duration_seconds,
			})
			.collect(),
	})
}
pub fn load(root: &Path, source: &AnimationSourceRef) -> Result<Arc<AnimationAsset>, String> {
	let (root, catalog) = root_and_catalog(root)?;
	let entry = catalog
		.by_id(&source.asset_id)
		.ok_or("Animation asset ID is missing from catalog")?;
	if entry.kind != AssetKind::Model {
		return Err("Animation requires a model asset".into());
	}
	let asset = load_path(&root, &entry.path)?;
	if (source.character.is_some() || asset.source_sha256 != source.expected_sha256)
		&& asset.source_sha256 != entry.fingerprint
	{
		return Err("Animation source changed outside its catalog; reimport it before adopting the new version".into());
	}
	asset.validate_source(source)?;
	Ok(asset)
}
pub fn load_path(root: &Path, relative: &str) -> Result<Arc<AnimationAsset>, String> {
	if !relative.to_ascii_lowercase().ends_with(".glb") {
		return Err("Animation subset requires an embedded GLB".into());
	}
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let bytes = read_bounded(&contained_file(&root, relative)?, MAX_ANIMATION_BYTES)?;
	decode(&bytes).map(Arc::new)
}
pub fn skinned_source(entity: &engine_core::scene_data::EntityData) -> Option<&AnimationSourceRef> {
	if let Some(animator) = entity
		.animator
		.as_ref()
		.filter(|component| component.enabled)
		&& let engine_core::animation::AnimatorBinding::Imported { source } =
			&animator.settings.binding
	{
		return Some(source);
	}
	entity
		.animation
		.as_ref()
		.map(|component| &component.settings.source)
}

pub fn prepare_scene(root: &Path, scene: &SceneData) -> Result<PreparedAnimationBindings, String> {
	prepare_scene_with_pins(root, scene, &BTreeMap::new())
}
/// Existing native models may pin a previously accepted version across unrelated
/// topology rebuilds. Identity/settings are still checked; only matching pins
/// bypass filesystem decoding. New worlds pass no pins and revalidate disk.
pub fn prepare_scene_with_pins(
	root: &Path,
	scene: &SceneData,
	pins: &PreparedAnimationBindings,
) -> Result<PreparedAnimationBindings, String> {
	let mut bindings = BTreeMap::new();
	let mut shared: BTreeMap<(AssetId, String), Arc<AnimationAsset>> = BTreeMap::new();
	if !scene
		.entities
		.iter()
		.any(|entity| skinned_source(entity).is_some())
	{
		return Ok(bindings);
	}
	let (_, catalog) = root_and_catalog(root)?;
	let mut retained = 0usize;
	for entity in &scene.entities {
		let Some(source) = skinned_source(entity) else {
			continue;
		};
		if bindings.len() >= MAX_ANIMATION_INSTANCES {
			return Err("Scene exceeds 64 animated instances".into());
		}
		let default;
		let s = if let Some(component) = entity
			.animation
			.as_ref()
			.filter(|component| &component.settings.source == source)
		{
			&component.settings
		} else {
			default = engine_core::animation::AnimationSettings {
				source: source.clone(),
				playback: engine_core::animation::AnimationPlayback::SingleClip {
					clip_index: 0,
					looping: false,
				},
				speed_numerator: 1,
				speed_denominator: 1,
			};
			&default
		};
		let resolved = if entity.mesh_id.is_some() {
			catalog.resolve_mesh_reference(&entity.model_reference()?)?
		} else {
			catalog
				.by_path(&entity.model_path)
				.map(|entry| (entry, entity.mesh_index.unwrap_or(0)))
		};
		let (entry, mesh_index) = resolved.ok_or("Animated mesh is not catalogued")?;
		if entry.id != s.source.asset_id
			|| entity
				.asset_id
				.as_ref()
				.is_some_and(|id| id != &s.source.asset_id)
		{
			return Err(
				"Mesh renderer and animation source have different asset identities".into(),
			);
		}
		let follows_character =
			s.source.character.is_some() && entry.fingerprint != s.source.expected_sha256;
		let revision = if follows_character {
			&entry.fingerprint
		} else {
			&s.source.expected_sha256
		};
		let key = (s.source.asset_id.clone(), revision.clone());
		let asset = match shared.get(&key) {
			Some(asset) => Arc::clone(asset),
			None => {
				let asset = match pins
					.get(&entity.key)
					.filter(|asset| &asset.source_sha256 == revision)
				{
					Some(asset) => asset.clone(),
					None => load(root, &s.source)?,
				};
				retained = retained
					.checked_add(asset.retained_bytes())
					.ok_or("Animation world byte overflow")?;
				if retained > 128 * 1024 * 1024 {
					return Err("World animation CPU assets exceed 128 MiB".into());
				}
				shared.insert(key, Arc::clone(&asset));
				asset
			}
		};
		asset.validate_settings(s)?;
		if mesh_index != asset.skin.mesh_index as usize {
			return Err("Animated mesh selection does not match skin mesh".into());
		}
		bindings.insert(entity.key.clone(), asset);
	}
	engine_logger::debug!(target: "bridge::animation", "Animation bindings prepared: bindings={}, assets={}, retained_bytes={retained}", bindings.len(), shared.len());
	Ok(bindings)
}
