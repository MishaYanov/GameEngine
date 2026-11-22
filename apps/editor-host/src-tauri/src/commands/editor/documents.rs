use super::*;

struct ProjectReplacement {
	expected_revision: Option<u64>,
	discard_unsaved: bool,
}

impl ProjectReplacement {
	fn validate(&self, current: Option<&EditorWorkspace>) -> Result<(), String> {
		if current.map(|workspace| workspace.backend.session().revision()) != self.expected_revision
		{
			return Err("Revision conflict: the open project changed. Review its current state and try again.".into());
		}
		if let Some(workspace) = current {
			let session = workspace.backend.session();
			if session.is_playing() {
				return Err("Stop Play before opening another project.".into());
			}
			if session.has_transaction() {
				return Err("Finish the current edit before opening another project.".into());
			}
			if session.is_dirty() && !self.discard_unsaved {
				return Err("The current project has unsaved changes. Confirm discarding them before opening another project.".into());
			}
		}
		Ok(())
	}
}

/// Prepare a candidate off the WebView thread, then install it through the
/// same revision, completion and viewport path for scene and project opening.
async fn open_workspace(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	replacement: Option<ProjectReplacement>,
	prepare: impl FnOnce(Box<dyn Fn() + Send>) -> Result<EditorWorkspace, String> + Send + 'static,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	let shared = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	tauri::async_runtime::spawn_blocking(move || {
		let notify_editor = Arc::clone(&shared);
		let notify_viewport = Arc::clone(&viewport);
		let notify_window = window.clone();
		let requested = std::time::Instant::now();
		let mut candidate = prepare(Box::new(move || {
			let editor = Arc::clone(&notify_editor);
			let viewport = Arc::clone(&notify_viewport);
			let window = notify_window.clone();
			tauri::async_runtime::spawn_blocking(move || {
				let completions = editor.lock().ok().and_then(|mut slot| slot.as_mut().map(EditorWorkspace::drain_asset_jobs)).unwrap_or_default();
				if !completions.is_empty() {
					let _ = window.emit("editor://asset-completions", &completions);
					if let Ok(viewport) = viewport.lock() { viewport.invalidate(); }
				}
			});
		}))?;
		let snapshot = {
			let mut current = shared.lock().map_err(|_| "Editor state lock failed")?;
			if current.as_ref().is_some_and(EditorWorkspace::has_dirty_clip_timelines) {
				return Err("Save or discard open Animation Clip changes before opening another project".into());
			}
			// Recheck under the installation lock: candidate preparation and native
			// dialogs must never authorize discarding edits made in the meantime.
			if let Some(replacement) = replacement {
				replacement.validate(current.as_ref())?;
			}
			if let Some(old) = current.as_ref() {
				candidate
					.backend
					.supersede_revision(old.backend.session().revision()).map_err(|e| e.to_string())?;
			}
			let ready = candidate.drain_asset_jobs();
			if !ready.is_empty() { let _ = window.emit("editor://asset-completions", ready); }
			let snapshot = candidate.backend.snapshot().map_err(|e| e.to_string())?;
			candidate.scene_switch = Some((snapshot.scene_revision, requested, std::time::Instant::now()));
			*current = Some(candidate);
			snapshot
		};
		viewport
			.lock()
			.map_err(|_| "Viewport state lock failed")?
			.invalidate();
		Ok(snapshot)
	})
		.await
		.map_err(|e| {
			if matches!(&e, tauri::Error::JoinError(join) if join.is_panic()) {
				engine_logger::debug!(target: "editor", "Editor open worker panicked; panic hook owns the error record");
			} else {
				engine_logger::error!(target: "editor", "Editor open worker cancelled");
			}
			format!("Editor worker failed: {e}")
		})?
}

#[tauri::command]
pub async fn editor_open(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	asset_root: String,
	scene_path: String,
) -> Result<EditorSnapshotDto, String> {
	open_workspace(window, state, None, move |on_ready| {
		EditorWorkspace::open_with_notifier(&asset_root, &scene_path, on_ready)
	})
	.await
}

#[tauri::command]
pub async fn editor_open_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	manifest_path: String,
	expected_revision: Option<u64>,
	discard_unsaved: Option<bool>,
) -> Result<EditorSnapshotDto, String> {
	let replacement = ProjectReplacement {
		expected_revision,
		discard_unsaved: discard_unsaved.unwrap_or(false),
	};
	open_workspace(window, state, Some(replacement), move |on_ready| {
		let location = ProjectLocation::from_manifest(std::path::Path::new(&manifest_path))
			.map_err(|error| error.to_string())?;
		EditorWorkspace::open_project_with_notifier(location, on_ready)
	})
	.await
}
#[tauri::command]
pub async fn editor_save_project(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_project"), move |w| {
		w.save_project(expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_save_all(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save_all"), move |w| {
		w.save_all(expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_save(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, false, Some("save"), move |w| {
		w.save(expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_reload(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: u64,
	policy: Option<ReloadPolicy>,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("reload"), move |w| {
		w.reload_with_policy(expected_revision, policy.unwrap_or_default())
	})
	.await
}

#[tauri::command]
pub async fn editor_register_scene(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("register_scene"), move |w| {
		w.register_scene(&path, expected_revision)
	})
	.await
}
#[tauri::command]
pub async fn editor_save_scene_as(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	path: String,
	expected_revision: u64,
) -> Result<EditorSnapshotDto, String> {
	trusted_window(&window)?;
	workspace(state, true, Some("save_scene_as"), move |w| {
		w.save_scene_as(&path, expected_revision)
	})
	.await
}
