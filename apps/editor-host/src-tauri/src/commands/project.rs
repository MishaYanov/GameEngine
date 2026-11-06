use serde::Serialize;
use std::path::PathBuf;
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
		window
			.dialog()
			.message("Opening another project will discard all unsaved scene and project changes.")
			.title("Discard unsaved changes?")
			.parent(&window)
			.kind(MessageDialogKind::Warning)
			.buttons(MessageDialogButtons::OkCancelCustom(
				"Discard and open".into(),
				"Keep current project".into(),
			))
			.blocking_show()
	})
	.await
	.map_err(|error| error.to_string())
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
