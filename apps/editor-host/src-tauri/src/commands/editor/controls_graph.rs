use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind, PersistentAsset},
	assets::project_assets,
	input::ControlGraph,
};

#[derive(Serialize)]
pub struct ControlsGraphAssetDto {
	asset: PersistentAsset,
	graph: ControlGraph,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlsGraphCaptureDto {
	id: AssetId,
	fingerprint: String,
}

fn check(workspace: &EditorWorkspace, generation: u64, revision: u64) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Controls Graph operation belongs to a closed project".into());
	}
	workspace.check_revision(revision)
}

#[tauri::command]
pub async fn editor_list_control_graphs(
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
			.filter(|asset| asset.kind == AssetKind::ControlsGraph)
			.collect())
	})
	.await
}

#[tauri::command]
pub async fn editor_open_control_graph(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<ControlsGraphAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		let asset = catalog
			.by_id(&asset_id)
			.ok_or("Controls Graph asset identity is unavailable")?;
		let graph = bridge::controls_graph::load(&current.asset_root, asset)?;
		Ok(ControlsGraphAssetDto {
			asset: asset.clone(),
			graph,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_save_control_graph(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	destination: String,
	capture: Option<ControlsGraphCaptureDto>,
	graph: ControlGraph,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<ControlsGraphAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_control_graph"), move |current| {
		check(current, expected_generation, expected_revision)?;
		if current.backend.session().is_playing() || current.backend.session().has_transaction() {
			return Err(
				"Finish the current edit and stop Play before saving the Controls Graph".into(),
			);
		}
		current.validate_asset_destination(&destination)?;
		let prepared = project_assets::prepare_control_graph(
			&current.asset_root,
			&destination,
			capture
				.as_ref()
				.map(|item| (&item.id, item.fingerprint.as_str())),
			&graph,
		)?;
		let asset = prepared.publish()?;
		Ok(ControlsGraphAssetDto { asset, graph })
	})
	.await
}
