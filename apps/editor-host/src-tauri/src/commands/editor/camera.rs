use super::*;
use crate::camera_preview::CameraPreviewDto;
use runtime::{camera_input::CameraIntent, scene_data::CameraData};

/// Only validation errors guarantee that the submitted camera or sample was never published.
#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum CameraPreviewError {
	Validation(String),
	Transport(String),
}

impl From<String> for CameraPreviewError {
	fn from(message: String) -> Self {
		Self::Transport(message)
	}
}

#[tauri::command]
pub async fn editor_camera_preview_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	camera: Option<CameraData>,
	camera_key: Option<String>,
	expected_revision: u64,
	expected_generation: u64,
	scene_id: String,
) -> Result<CameraPreviewDto, CameraPreviewError> {
	trusted_window(&window).map_err(CameraPreviewError::Validation)?;
	workspace_with_error(state, true, Some("camera_preview_open"), move |workspace| {
		match (camera, camera_key) {
			(Some(camera), None) => workspace.open_camera_preview(
				camera,
				expected_revision,
				expected_generation,
				&scene_id,
			),
			(None, Some(key)) if key.len() <= 256 && !key.chars().any(char::is_control) => {
				let key = runtime::EntityKey::new(key).map_err(CameraPreviewError::Validation)?;
				workspace.open_selected_camera_preview(
					key,
					expected_revision,
					expected_generation,
					&scene_id,
				)
			}
			_ => Err("Choose a valid Camera object or a camera draft for this preview".into()),
		}
		.map_err(CameraPreviewError::Validation)
	})
	.await
}

#[tauri::command]
pub async fn editor_camera_preview_sample(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
	next_sample: u64,
	intent: CameraIntent,
) -> Result<CameraPreviewDto, CameraPreviewError> {
	trusted_window(&window).map_err(CameraPreviewError::Validation)?;
	check_token(&token).map_err(CameraPreviewError::Validation)?;
	workspace_with_error(
		state,
		true,
		Some("camera_preview_sample"),
		move |workspace| {
			workspace
				.sample_camera_preview(&token, next_sample, intent)
				.map_err(CameraPreviewError::Validation)
		},
	)
	.await
}

#[tauri::command]
pub async fn editor_camera_preview_close(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	token: String,
) -> Result<(), String> {
	trusted_window(&window)?;
	check_token(&token)?;
	workspace(state, false, None, move |workspace| {
		workspace.close_camera_preview(&token);
		Ok(())
	})
	.await
}

fn check_token(token: &str) -> Result<(), String> {
	if token.is_empty() || token.len() > 128 {
		return Err("Invalid camera preview token".into());
	}
	Ok(())
}
