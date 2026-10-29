use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use tauri_plugin_dialog::DialogExt;

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
	launched: bool,
	launch_error: Option<String>,
}

#[tauri::command]
pub async fn create_project(name: String, destination: String) -> Result<ProjectResult, String> {
	tauri::async_runtime::spawn_blocking(move || {
		let path = compiler::install_project(&name, &destination)?;
		let executable = path.join(format!("game{}", std::env::consts::EXE_SUFFIX));
		let launch = Command::new(executable).current_dir(&path).spawn();
		Ok(ProjectResult {
			path: path.to_string_lossy().into_owned(),
			launched: launch.is_ok(),
			launch_error: launch.err().map(|error| error.to_string()),
		})
	})
	.await
	.map_err(|error| error.to_string())?
}

#[cfg(test)]
#[path = "tests_project.rs"]
mod tests;
