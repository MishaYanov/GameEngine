use super::*;

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
