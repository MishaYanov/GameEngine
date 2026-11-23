use super::*;
use runtime::{
	asset_catalog::{AssetId, AssetKind, PersistentAsset},
	assets::project_assets,
	input::InputActionMap,
};

#[derive(Serialize)]
pub struct InputActionAssetDto {
	asset: PersistentAsset,
	map: InputActionMap,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputActionCaptureDto {
	id: AssetId,
	fingerprint: String,
}

fn check(workspace: &EditorWorkspace, generation: u64, revision: u64) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Input Actions operation belongs to a closed project".into());
	}
	workspace.check_revision(revision)
}

#[tauri::command]
pub async fn editor_list_input_actions(
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
			.filter(|asset| asset.kind == AssetKind::InputActions)
			.collect())
	})
	.await
}

#[tauri::command]
pub async fn editor_open_input_actions(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_id: AssetId,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<InputActionAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, move |current| {
		check(current, expected_generation, expected_revision)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		let asset = catalog
			.by_id(&asset_id)
			.ok_or("Input Actions identity is unavailable")?;
		let map = project_assets::load_input_actions(&current.asset_root, asset)?;
		Ok(InputActionAssetDto {
			asset: asset.clone(),
			map,
		})
	})
	.await
}

#[tauri::command]
pub async fn editor_save_input_actions(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	destination: String,
	capture: Option<InputActionCaptureDto>,
	map: InputActionMap,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<InputActionAssetDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_input_actions"), move |current| {
		check(current, expected_generation, expected_revision)?;
		if current.backend.session().is_playing() || current.backend.session().has_transaction() {
			return Err("Finish the current edit and stop Play before saving Input Actions".into());
		}
		current.validate_asset_destination(&destination)?;
		map.validate_scenes(&current.backend.session().project().scenes)?;
		let prepared = project_assets::prepare_input_actions(
			&current.asset_root,
			&destination,
			capture
				.as_ref()
				.map(|item| (&item.id, item.fingerprint.as_str())),
			&map,
		)?;
		let asset = prepared.publish()?;
		Ok(InputActionAssetDto { asset, map })
	})
	.await
}

#[tauri::command]
pub async fn editor_move_input_actions(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	destination: String,
	capture: InputActionCaptureDto,
	expected_generation: u64,
	expected_revision: u64,
) -> Result<PersistentAsset, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("move_input_actions"), move |current| {
		check(current, expected_generation, expected_revision)?;
		if current.backend.session().is_playing() || current.backend.session().has_transaction() {
			return Err("Finish the current edit and stop Play before moving Input Actions".into());
		}
		current.validate_asset_destination(&destination)?;
		let catalog = project_assets::load_catalog(&current.asset_root)?;
		let asset = catalog
			.by_id(&capture.id)
			.ok_or("Input Actions identity is unavailable")?;
		if asset.kind != AssetKind::InputActions || asset.fingerprint != capture.fingerprint {
			return Err(
				"Input Actions changed after opening; reopen the asset before moving".into(),
			);
		}
		let prepared =
			project_assets::prepare_move(&current.asset_root, &capture.id, &destination)?;
		if prepared.previous_fingerprint() != Some(capture.fingerprint.as_str()) {
			return Err("Input Actions changed while preparing the move".into());
		}
		prepared.publish()
	})
	.await
}
