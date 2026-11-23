//! Asset and per-actor Animator binding preparation shared by authoring and runtime.
use crate::asset_io::{contained_file, is_link, read_bounded};
use engine_core::{
	SceneData,
	animation::{
		AnimationClipRef, AnimationPlayback, AnimationSettings, AnimatorBinding, AnimatorGraph,
		AnimatorSettings, AuthoredClip, MAX_ANIMATOR_GRAPH_BYTES, MAX_AUTHORED_CLIP_BYTES,
	},
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES, PersistentAsset,
	},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::EntityData,
};
use sha2::{Digest, Sha256};
use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

use engine_core::animation::PreparedAnimatorBindings;
pub use engine_core::animation::{PreparedAnimator, PreparedAnimatorClips};

fn root(root: &Path) -> Result<PathBuf, String> {
	let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Animator asset root must be an ordinary directory".into());
	}
	root.canonicalize().map_err(|error| error.to_string())
}

fn verified(
	root: &Path,
	asset: &PersistentAsset,
	kind: AssetKind,
	limit: usize,
) -> Result<Vec<u8>, String> {
	asset.validate()?;
	if asset.kind != kind || asset.bytes > limit as u64 {
		return Err(format!(
			"Asset {:?} has the wrong kind or exceeds its byte limit",
			asset.path
		));
	}
	let bytes = read_bounded(&contained_file(root, &asset.path)?, limit)?;
	if bytes.len() as u64 != asset.bytes
		|| format!("{:x}", Sha256::digest(&bytes)) != asset.fingerprint
	{
		return Err(format!(
			"Animator dependency {:?} changed outside its catalog",
			asset.path
		));
	}
	Ok(bytes)
}

pub fn load(root_path: &Path, asset: &PersistentAsset) -> Result<AnimatorGraph, String> {
	AnimatorGraph::from_json(&verified(
		&root(root_path)?,
		asset,
		AssetKind::AnimatorGraph,
		MAX_ANIMATOR_GRAPH_BYTES,
	)?)
}

pub fn prepare_actor(
	root_path: &Path,
	project: &ProjectData,
	scene_id: &SceneId,
	scene: &SceneData,
	actor: &EntityData,
	settings: &AnimatorSettings,
) -> Result<PreparedAnimator, String> {
	let root = root(root_path)?;
	let raw = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
	let catalog =
		AssetCatalog::from_json(std::str::from_utf8(&raw).map_err(|error| error.to_string())?)?;
	let asset = catalog
		.by_id(&settings.asset_id)
		.ok_or("Animator asset identity is missing")?;
	let graph = Arc::new(load(&root, asset)?);
	graph.validate_binding(&settings.binding)?;
	let clips = match &settings.binding {
		AnimatorBinding::Imported { source } => {
			let rendered =
				catalog.resolve(actor.asset_id.as_ref(), &actor.model_path, actor.mesh_index)?;
			if !actor.renderable
				|| !actor.has_mesh_component()
				|| rendered.is_none_or(|model| model.id != source.asset_id)
			{
				return Err("Animator rig must be the rendered model on the same actor".into());
			}
			let asset = crate::animation_asset::load(&root, source)?;
			if actor.mesh_index.unwrap_or(0) != asset.skin.mesh_index as usize {
				return Err("Animator rig targets a different mesh selection".into());
			}
			for state in &graph.states {
				let AnimationClipRef::Imported { clip_index, .. } = &state.clip else {
					return Err("Skeletal Animator state requires an imported clip".into());
				};
				asset
					.validate_settings(&AnimationSettings {
						source: source.clone(),
						playback: AnimationPlayback::SingleClip {
							clip_index: *clip_index,
							looping: state.looping,
						},
						speed_numerator: 1,
						speed_denominator: 1,
					})
					.map_err(|error| format!("Animator state {:?}: {error}", state.id))?;
			}
			PreparedAnimatorClips::Imported(asset)
		}
		AnimatorBinding::Visual { targets } => {
			let mut cache = BTreeMap::<AssetId, AuthoredClip>::new();
			let mut clips = Vec::with_capacity(graph.states.len());
			for state in &graph.states {
				let AnimationClipRef::Authored {
					asset_id,
					expected_sha256,
				} = &state.clip
				else {
					return Err("Visual Animator state requires an authored clip".into());
				};
				let asset = catalog.by_id(asset_id).ok_or_else(|| {
					format!(
						"Animator state {:?} references a missing Animation Clip",
						state.id
					)
				})?;
				if &asset.fingerprint != expected_sha256 {
					return Err(format!(
						"Animator state {:?} pins a different Animation Clip revision",
						state.id
					));
				}
				if !cache.contains_key(asset_id) {
					let bytes = verified(
						&root,
						asset,
						AssetKind::AnimationClip,
						MAX_AUTHORED_CLIP_BYTES,
					)?;
					cache.insert(asset_id.clone(), AuthoredClip::from_json(&bytes)?);
				}
				let resolved = cache[asset_id]
					.resolve_animator(project, scene_id, scene, &actor.key, targets)
					.map_err(|error| format!("Animator state {:?}: {error}", state.id))?;
				clips.push(resolved);
			}
			PreparedAnimatorClips::Visual(clips)
		}
	};
	let prepared = PreparedAnimator { graph, clips };
	if let Some(engine_core::components::AudioEventBinding::AnimationMarker { clip, marker }) =
		actor
			.audio_source
			.as_ref()
			.filter(|source| source.enabled)
			.and_then(|source| source.settings.event.as_ref())
	{
		let found =
			match &prepared.clips {
				PreparedAnimatorClips::Imported(_) => false,
				PreparedAnimatorClips::Visual(clips) => prepared
					.graph
					.states
					.iter()
					.zip(clips)
					.any(|(state, authored)| {
						matches!(&state.clip, AnimationClipRef::Authored { asset_id, .. } if asset_id == clip)
							&& authored.markers().iter().any(|value| &value.id == marker)
					}),
			};
		if !found {
			return Err("Audio Source references a missing marker. Open Animation and choose a saved marker from this Animator.".into());
		}
	}
	Ok(prepared)
}

pub fn prepare_scene(
	root: &Path,
	project: &ProjectData,
	scene_id: &SceneId,
	scene: &SceneData,
) -> Result<PreparedAnimatorBindings, String> {
	let mut bindings = PreparedAnimatorBindings::new();
	let mut retained = 0usize;
	for actor in &scene.entities {
		if let Some(event) = actor
			.audio_source
			.as_ref()
			.filter(|source| source.enabled)
			.and_then(|source| source.settings.event.as_ref())
		{
			use engine_core::components::{AudioEventBinding, NativeBehaviorSpec};
			let issue = match event {
				AudioEventBinding::AnimationMarker { .. }
					if actor
						.animator
						.as_ref()
						.is_none_or(|animator| !animator.enabled) =>
				{
					Some(
						"Animation marker audio requires an enabled Animator on this object. Open Animation.",
					)
				}
				AudioEventBinding::GameplaySignal { .. }
					if !actor.native_behaviors.iter().any(|entry| {
						entry.enabled
							&& matches!(entry.behavior, NativeBehaviorSpec::StateMachine { .. })
					}) =>
				{
					Some("Gameplay signal audio requires an enabled state machine on this object.")
				}
				_ => None,
			};
			if let Some(issue) = issue {
				return Err(format!("Audio Source on {:?}: {issue}", actor.key.as_str()));
			}
		}
		let Some(component) = actor.animator.as_ref().filter(|animator| animator.enabled) else {
			continue;
		};
		if bindings.len() == 64 {
			return Err("A Play scene supports at most 64 Animator actors".into());
		}
		let prepared = prepare_actor(root, project, scene_id, scene, actor, &component.settings)
			.map_err(|error| format!("Animator on {:?}: {error}", actor.key.as_str()))?;
		let bytes = match &prepared.clips {
			PreparedAnimatorClips::Imported(asset) => asset.retained_bytes(),
			PreparedAnimatorClips::Visual(clips) => {
				clips.iter().map(|clip| clip.retained_bytes()).sum()
			}
		};
		retained = retained
			.checked_add(bytes)
			.filter(|bytes| *bytes <= 128 * 1024 * 1024)
			.ok_or("Animator CPU assets exceed the 128 MiB scene budget")?;
		bindings.insert(actor.key.clone(), Arc::new(prepared));
	}
	Ok(bindings)
}
