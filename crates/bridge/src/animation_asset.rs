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
pub struct AnimationMetadata {
	pub source: AnimationSourceRef,
	pub mesh_index: u32,
	pub nodes: usize,
	pub joints: usize,
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
	Ok(AnimationMetadata {
		source: AnimationSourceRef {
			asset_id: id.clone(),
			expected_sha256: asset.source_sha256.clone(),
			scene_index: asset.source_scene,
			mesh_node: asset.skin.source_mesh_node,
			skin_index: asset.skin.source_skin,
		},
		mesh_index: asset.skin.mesh_index,
		nodes: asset.nodes.len(),
		joints: asset.skin.joints.len(),
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
	if asset.source_sha256 != source.expected_sha256
		|| asset.source_scene != source.scene_index
		|| asset.skin.source_mesh_node != source.mesh_node
		|| asset.skin.source_skin != source.skin_index
	{
		return Err("Animation source changed; review and adopt its current version".into());
	}
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
		let entry = catalog
			.by_path(&entity.model_path)
			.ok_or("Animated mesh is not catalogued")?;
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
		let key = (s.source.asset_id.clone(), s.source.expected_sha256.clone());
		let asset = match shared.get(&key) {
			Some(asset) => Arc::clone(asset),
			None => {
				let asset = match pins
					.get(&entity.key)
					.filter(|asset| asset.validate_settings(s).is_ok())
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
		if entity.mesh_index.unwrap_or(0) != asset.skin.mesh_index as usize {
			return Err("Animated mesh selection does not match skin mesh".into());
		}
		bindings.insert(entity.key.clone(), asset);
	}
	engine_logger::debug!(target: "bridge::animation", "Animation bindings prepared: bindings={}, assets={}, retained_bytes={retained}", bindings.len(), shared.len());
	Ok(bindings)
}
