use super::*;
use runtime::{
	animation::AnimatorGraph,
	asset_catalog::{AssetId, AssetKind, PersistentAsset},
	assets::project_assets,
	EntityKey,
};

#[derive(Serialize)]
pub struct AnimatorAssetDto {
	asset: PersistentAsset,
	graph: AnimatorGraph,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatorCaptureDto {
	id: AssetId,
	fingerprint: String,
}

#[derive(Serialize)]
pub struct AnimatorValidationDto {
	entry: String,
	states: usize,
	parameters: usize,
	runtime_digest: String,
}

fn check(workspace: &EditorWorkspace, generation: u64, revision: u64) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Animator operation belongs to a closed project".into());
	}
	workspace.check_revision(revision)
}

#[tauri::command]
pub async fn editor_list_animators(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<Vec<PersistentAsset>, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		Ok(catalog
			.assets
			.into_iter()
			.filter(|asset| asset.kind == AssetKind::AnimatorGraph)
			.collect())
	})
	.await
}

#[tauri::command]
pub async fn editor_open_animator(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<AnimatorAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		let asset = catalog
			.by_id(&asset_id)
			.ok_or("Animator asset identity is unavailable")?;
		let graph = bridge::animator_graph::load(&current.asset_root, asset)?;
		Ok(AnimatorAssetDto {
			asset: asset.clone(),
			graph,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_save_animator(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	destination: String,
	capture: Option<AnimatorCaptureDto>,
	graph: AnimatorGraph,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<AnimatorAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_animator"), move |current| {
		check(current, expected_generation, expected_revision)?;
		if current.backend.session().is_playing() || current.backend.session().has_transaction() {
			return Err("Finish the current edit and stop Play before saving the Animator".into());
		}
		current.validate_asset_destination(&destination)?;
		let prepared = project_assets::prepare_animator(
			&current.asset_root,
			&destination,
			&graph,
			capture
				.as_ref()
				.map(|item| (&item.id, item.fingerprint.as_str())),
		)?;
		let asset = prepared.publish()?;
		Ok(AnimatorAssetDto { asset, graph })
	})
	.await
}

#[tauri::command]
pub async fn editor_validate_animator_actor(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	entity_key: EntityKey,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<AnimatorValidationDto, String> {
	trusted_window(&window)?;
	workspace(
		state,
		false,
		Some("validate_animator_actor"),
		move |current| {
			check(current, expected_generation, expected_revision)?;
			let session = current.backend.session();
			let scene = session.authored_document();
			let actor = scene
				.entities
				.iter()
				.find(|entity| entity.key == entity_key)
				.ok_or("Animator actor no longer exists in this scene")?;
			let component = actor
				.animator
				.as_ref()
				.ok_or("Selected object has no Animator")?;
			let prepared = bridge::animator_graph::prepare_actor(
				&current.asset_root,
				session.project(),
				session.active_scene_id(),
				&scene,
				actor,
				&component.settings,
			)
			.map_err(|cause| format!("Animator on {:?}: {cause}", actor.key.as_str()))?;
			Ok(AnimatorValidationDto {
				entry: prepared.graph.entry.clone(),
				states: prepared.graph.states.len(),
				parameters: prepared.graph.parameters.len(),
				runtime_digest: prepared.graph.runtime_digest()?,
			})
		},
	)
	.await
}
