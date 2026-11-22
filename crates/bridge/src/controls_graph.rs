//! Confined Controls Graph assets and per-actor semantic input validation.
use crate::{
	asset_io::{contained_file, read_bounded},
	input_actions::{canonical_root, read_catalog},
};
use engine_core::{
	SceneData,
	asset_catalog::{AssetCatalog, AssetKind, PersistentAsset},
	components::InputActionsRef,
	input::{
		CompiledControlGraph, ControlGraph, ControlNodeData, InputActionMap,
		MAX_CONTROL_GRAPH_BYTES, control_action_types,
	},
	project::ProjectData,
	scene_data::EntityData,
};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn load(root: &Path, asset: &PersistentAsset) -> Result<ControlGraph, String> {
	asset.validate()?;
	if asset.kind != AssetKind::ControlsGraph {
		return Err("Player Controller requires a Controls Graph asset".into());
	}
	let root = canonical_root(root)?;
	let bytes = read_bounded(
		&contained_file(&root, &asset.path)?,
		MAX_CONTROL_GRAPH_BYTES,
	)?;
	if bytes.len() as u64 != asset.bytes
		|| format!("{:x}", Sha256::digest(&bytes)) != asset.fingerprint
	{
		return Err(format!(
			"Controls Graph {:?} changed outside its catalog",
			asset.path
		));
	}
	ControlGraph::from_json(&bytes)
}

pub fn compile_for_actor(
	root: &Path,
	project: &ProjectData,
	actor: Option<&EntityData>,
	graph: &ControlGraph,
) -> Result<CompiledControlGraph, String> {
	let root = canonical_root(root)?;
	let catalog = read_catalog(&root)?;
	compile(&root, &catalog, project, actor, graph, None)
}

/// Validate every enabled declaration, then return the local player's routing plan.
pub fn prepare_scene(
	root: &Path,
	project: &ProjectData,
	scene: &SceneData,
	local_map: Option<&InputActionMap>,
) -> Result<Option<CompiledControlGraph>, String> {
	let mut actors = scene
		.entities
		.iter()
		.filter_map(|actor| {
			let controller = actor
				.player_controller
				.as_ref()
				.filter(|component| component.enabled)?;
			Some((actor, controller.settings.control_graph.as_ref()?))
		})
		.peekable();
	if actors.peek().is_none() {
		return Ok(None);
	}
	let root = canonical_root(root)?;
	let catalog = read_catalog(&root)?;
	let mut local = None;
	for (actor, id) in actors {
		let prepared = (|| {
			let asset = catalog.by_id(id).ok_or_else(|| {
				format!("Controls Graph asset identity {} is missing", id.as_str())
			})?;
			let graph = load(&root, asset)?;
			let map = if actor.is_local_player() {
				local_map
			} else {
				None
			};
			compile(&root, &catalog, project, Some(actor), &graph, map)
		})()
		.map_err(|error| {
			format!(
				"Player Controller on {:?}, Controls Graph: {error}",
				actor.key.as_str()
			)
		})?;
		if actor.is_local_player() {
			local = Some(prepared);
		}
	}
	Ok(local)
}

fn compile(
	root: &Path,
	catalog: &AssetCatalog,
	project: &ProjectData,
	actor: Option<&EntityData>,
	graph: &ControlGraph,
	map: Option<&InputActionMap>,
) -> Result<CompiledControlGraph, String> {
	let profile = project.input_profile.clone().unwrap_or_default();
	let settings = actor
		.and_then(|actor| actor.player_controller.as_ref())
		.map(|component| &component.settings);
	let mut loaded_map = None;
	if map.is_none()
		&& let Some(InputActionsRef::Asset { asset_id }) = settings.map(|settings| &settings.input)
	{
		let asset = catalog
			.by_id(asset_id)
			.ok_or("Player Controller Input Actions identity is unavailable")?;
		loaded_map = Some(crate::input_actions::load(root, asset)?);
	}
	let actions = control_action_types(map.or(loaded_map.as_ref()), &profile);
	let animator = if graph
		.nodes
		.iter()
		.any(|node| matches!(node.data, ControlNodeData::AnimatorBool { .. }))
		&& let Some(component) = actor
			.and_then(|actor| actor.animator.as_ref())
			.filter(|component| component.enabled)
	{
		let asset = catalog
			.by_id(&component.settings.asset_id)
			.ok_or("Actor Animator asset identity is unavailable")?;
		Some(crate::animator_graph::load(root, asset)?)
	} else {
		None
	};
	graph.compile(&actions, animator.as_ref())
}
