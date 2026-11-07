use crate::{editor::EditorWorkspace, state::EditorState};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

#[tauri::command]
pub async fn choose_project_manifest(
	window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Open project.json")
			.add_filter("Project manifest", &["json"])
			.set_file_name("project.json")
			.blocking_pick_file()
			.map(|file| {
				file.into_path()
					.map(|path| path.to_string_lossy().into_owned())
					.map_err(|error| error.to_string())
			})
			.transpose()
	})
	.await
	.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn confirm_project_discard(window: tauri::WebviewWindow) -> Result<bool, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		confirm_discard(
			&window,
			"Opening another project will discard all unsaved scene and project changes.",
			"Discard and open",
			"Keep current project",
		)
	})
		.await
		.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn editor_exit(
	window: tauri::WebviewWindow,
	state: State<'_, EditorState>,
	expected_revision: Option<u64>,
) -> Result<bool, String> {
	super::editor::trusted_window(&window)?;
	let editor = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let review = |workspace: Option<&EditorWorkspace>| -> Result<bool, String> {
			let session = workspace.map(|workspace| workspace.backend.session());
			if session.map(|session| session.revision()) != expected_revision {
				return Err("Revision conflict: the open project changed. Try Exit again.".into());
			}
			if session.is_some_and(|session| session.has_transaction()) {
				return Err("Finish the current edit before exiting.".into());
			}
			Ok(session.is_some_and(|session| session.is_dirty()))
		};
		let dirty = {
			let workspace = editor.lock().map_err(|_| "Editor state lock failed")?;
			review(workspace.as_ref())?
		};
		if dirty
			&& !confirm_discard(
			&window,
			"Exiting the editor will discard all unsaved scene and project changes.",
			"Discard and exit",
			"Keep editing",
		) {
			return Ok(false);
		}

		let mut workspace = editor.lock().map_err(|_| "Editor state lock failed")?;
		if review(workspace.as_ref())? && !dirty {
			return Err(
				"Revision conflict: the project has new unsaved changes. Try Exit again.".into(),
			);
		}
		// CloseRequested already waits for the native viewport before destroying its parent.
		window.close().map_err(|error| error.to_string())?;
		// Retire the reviewed workspace before background completions can adopt more edits.
		let retired = workspace.take();
		drop(workspace);
		drop(retired);
		Ok(true)
	})
		.await
		.map_err(|error| error.to_string())?
}

fn confirm_discard(
	window: &tauri::WebviewWindow,
	message: &str,
	accept: &str,
	cancel: &str,
) -> bool {
	window
		.dialog()
		.message(message)
		.title("Discard unsaved changes?")
		.parent(window)
		.kind(MessageDialogKind::Warning)
		.buttons(MessageDialogButtons::OkCancelCustom(
			accept.into(),
			cancel.into(),
		))
		.blocking_show()
}

#[tauri::command]
pub async fn choose_project_destination(
	window: tauri::WebviewWindow,
	initial_directory: Option<String>,
) -> Result<Option<String>, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		let mut dialog = window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Choose a project destination folder");
		if let Some(directory) = initial_directory {
			let path = PathBuf::from(directory.trim());
			if path.is_absolute() && path.is_dir() {
				dialog = dialog.set_directory(path);
			}
		}
		dialog
			.blocking_pick_folder()
			.map(|folder| {
				folder
					.into_path()
					.map(|path| path.to_string_lossy().into_owned())
					.map_err(|error| error.to_string())
			})
			.transpose()
	})
	.await
	.map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
	path: String,
	manifest_path: String,
}

#[tauri::command]
pub async fn create_project(
	window: tauri::WebviewWindow,
	name: String,
	destination: String,
) -> Result<ProjectResult, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		let path = compiler::create_authoring_project(&name, &destination)?;
		Ok(ProjectResult {
			manifest_path: path.join("project.json").to_string_lossy().into_owned(),
			path: path.to_string_lossy().into_owned(),
		})
	})
	.await
	.map_err(|error| error.to_string())?
}

#[cfg(test)]
#[path = "tests_project.rs"]
mod tests;
