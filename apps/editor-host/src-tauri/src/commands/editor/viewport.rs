use super::*;
use crate::editor::scene_warming::publish_warming_report;
use bridge::viewport::SceneWarmingMode;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct CollisionSnapshotDto {
	contacts: Vec<Contact>,
	shapes: Vec<DebugShape>,
}
#[tauri::command]
pub async fn editor_collisions(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<CollisionSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, None, |w| {
		let document = w.backend.session().render_document();
		Ok(CollisionSnapshotDto {
			contacts: runtime::collision::query_overlaps(&document)?,
			shapes: runtime::collision::debug_shapes(&document)?,
		})
	})
		.await
}

#[tauri::command]
pub async fn editor_warm_scene(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	expected_revision: u64,
	expected_mode_revision: u64,
	scene_id: Option<runtime::scene_catalog::SceneId>,
) -> Result<(), String> {
	trusted_window(&window)?;
	if !cfg!(target_os = "windows") {
		return Err("Scene warming requires the Windows editor viewport".into());
	}
	workspace(state, false, None, move |editor| {
		editor.warm_scene(
			expected_generation,
			expected_revision,
			expected_mode_revision,
			scene_id,
		)
	})
	.await
}

#[tauri::command]
pub async fn editor_set_scene_warming_mode(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
	mode: SceneWarmingMode,
) -> Result<u64, String> {
	trusted_window(&window)?;
	if !cfg!(target_os = "windows") {
		return Err("Scene warming requires the Windows editor viewport".into());
	}
	workspace(state, true, None, move |editor| {
		editor.set_scene_warming_mode(expected_generation, mode)
	})
		.await
}

#[tauri::command]
pub async fn editor_export_warming_report(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_generation: u64,
) -> Result<Option<usize>, String> {
	trusted_window(&window)?;
	if !cfg!(target_os = "windows") {
		return Err("Scene warming requires the Windows editor viewport".into());
	}
	let shared = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		{
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			slot.as_ref()
				.ok_or("Open a scene before exporting a warming report")?
				.check_warming_export(expected_generation)?;
		}
		let Some(file) = window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Export Warming Report")
			.set_file_name("warming-report.json")
			.add_filter("Warming report", &["json"])
			.blocking_save_file()
		else {
			return Ok(None);
		};
		trusted_window(&window)?;
		let destination = file.into_path().map_err(|error| error.to_string())?;
		if !destination.is_absolute()
			|| !destination
			.extension()
			.is_some_and(|suffix| suffix.eq_ignore_ascii_case("json"))
		{
			return Err("Choose an absolute .json filename for the warming report".into());
		}
		let session_id = engine_logger::snapshot(engine_logger::SnapshotRequest {
			limit: 0,
			..Default::default()
		})
			.session_id
			.ok_or("The native log session is unavailable")?;
		let report = {
			let slot = shared.lock().map_err(|_| "Editor state lock failed")?;
			slot.as_ref()
				.ok_or("Workspace closed before the warming report could be exported")?
				.capture_warming_report(expected_generation, session_id)?
		};
		publish_warming_report(&report, &destination).map(Some)
	})
		.await
		.map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn editor_viewport_attach(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	bounds: ViewportBoundsDto,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.attach(&window, bounds)
}
#[tauri::command]
pub fn editor_viewport_bounds(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	bounds: ViewportBoundsDto,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.bounds(bounds)
}
#[tauri::command]
pub fn editor_viewport_detach(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	if let Some(editor) = state
		.editor
		.lock()
		.map_err(|_| "Editor state lock failed")?
		.as_ref()
	{
		editor.scene_warming.cancel();
	}
	let mut viewport = state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?;
	viewport.detach();
	Ok(viewport.status())
}
#[tauri::command]
pub fn editor_viewport_status(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<ViewportStatusDto, String> {
	trusted_window(&window)?;
	Ok(state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.status())
}

#[tauri::command]
pub fn editor_viewport_editor_state(
	window: WebviewWindow,
	state: State<'_, EditorState>,
) -> Result<bridge::viewport::ViewportEditorStateDto, String> {
	trusted_window(&window)?;
	state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.editor_state()
}

#[tauri::command]
pub async fn editor_viewport_action(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	action: bridge::viewport::ViewportActionDto,
) -> Result<bridge::viewport::ViewportEditorStateDto, String> {
	trusted_window(&window)?;
	let receiver = state
		.viewport
		.lock()
		.map_err(|_| "Viewport state lock failed")?
		.request_action(action)?;
	tauri::async_runtime::spawn_blocking(move || {
		receiver
			.recv_timeout(std::time::Duration::from_secs(9))
			.map_err(|_| {
				"Viewport action did not complete; inspect its current state before retrying"
					.to_string()
			})?
	})
	.await
	.map_err(|error| format!("Viewport action worker failed: {error}"))?
}
