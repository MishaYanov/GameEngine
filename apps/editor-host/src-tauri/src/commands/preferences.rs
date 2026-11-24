use crate::{
	layout_preferences::{self, LayoutPreferences},
	state::EditorState,
};
use sha2::{Digest, Sha256};
use std::{
	ffi::OsString,
	path::{Path, PathBuf},
	sync::Arc,
};
use tauri::{Manager, State, WebviewWindow};

const CONFIG_DIRECTORY_ENV: &str = "ENGINE_EDITOR_CONFIG_DIR";

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
	layout_path(std::env::var_os(CONFIG_DIRECTORY_ENV), || {
		app.path()
			.app_config_dir()
			.map_err(|error| error.to_string())
	})
}

fn layout_path(
	override_directory: Option<OsString>,
	default_directory: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
	let directory = match override_directory {
		Some(path) => {
			let path = PathBuf::from(path);
			if !path.is_absolute() {
				return Err(format!(
					"{CONFIG_DIRECTORY_ENV} must be a nonempty absolute path"
				));
			}
			path
		}
		None => default_directory()?,
	};
	Ok(directory.join("editor-layout.json"))
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

/// Return opaque identity for the project occupying this native workspace generation.
#[tauri::command]
pub async fn editor_layout_preferences_project_scope(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	workspace_generation: u64,
) -> Result<String, String> {
	super::editor::trusted_window(&window)?;
	let editor = Arc::clone(&state.editor);
	tauri::async_runtime::spawn_blocking(move || {
		let slot = editor.lock().map_err(|_| "Editor state lock failed")?;
		let workspace = slot
			.as_ref()
			.filter(|workspace| workspace.generation == workspace_generation)
			.ok_or("Open project changed while resolving layout preferences")?;
		Ok(project_scope(&workspace.project_path))
	})
	.await
	.map_err(|error| error.to_string())?
}

fn project_scope(manifest_path: &Path) -> String {
	// EditorWorkspace already resolved the path when the project was opened.
	// Native path bytes preserve distinct non-Unicode paths without exposing them.
	format!(
		"{:x}",
		Sha256::digest(manifest_path.as_os_str().as_encoded_bytes())
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn project_scope_is_stable_and_keeps_same_named_projects_separate() {
		let root = std::env::current_dir().unwrap();
		let first = root.join("first/game/project.json");
		let second = root.join("second/game/project.json");
		let identity = project_scope(&first);
		assert_eq!(identity, project_scope(&first));
		assert_ne!(identity, project_scope(&second));
		assert_eq!(identity.len(), 64);
		assert!(identity
			.chars()
			.all(|character| character.is_ascii_hexdigit()));
	}

	#[test]
	fn layout_override_uses_absolute_path_without_resolving_platform_defaults() {
		let directory = std::env::current_dir().unwrap().join("editor-preferences");
		let overridden = layout_path(Some(directory.clone().into_os_string()), || {
			panic!("an explicit override should be used before platform defaults")
		})
		.unwrap();
		assert_eq!(overridden, directory.join("editor-layout.json"));
	}

	#[test]
	fn layout_defaults_when_unset_and_rejects_empty_or_relative_overrides() {
		let default = std::env::current_dir().unwrap();
		assert_eq!(
			layout_path(None, || Ok(default.clone())).unwrap(),
			default.join("editor-layout.json")
		);
		for invalid in ["", "relative"] {
			assert!(layout_path(Some(invalid.into()), || Ok(default.clone())).is_err());
		}
	}
}
