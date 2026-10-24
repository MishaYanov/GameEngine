use crate::layout_preferences::{self, LayoutPreferences};
use std::path::PathBuf;
use tauri::Manager;

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
	app.path()
		.app_config_dir()
		.map(|directory| directory.join("editor-layout.json"))
		.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn editor_layout_preferences_load(
	app: tauri::AppHandle,
) -> Result<Option<LayoutPreferences>, String> {
	let path = preferences_path(&app)?;
	tauri::async_runtime::spawn_blocking(move || layout_preferences::load(&path))
		.await
		.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn editor_layout_preferences_save(
	app: tauri::AppHandle,
	preferences: LayoutPreferences,
) -> Result<(), String> {
	let path = preferences_path(&app)?;
	tauri::async_runtime::spawn_blocking(move || layout_preferences::save(&path, &preferences))
		.await
		.map_err(|error| error.to_string())?
}
