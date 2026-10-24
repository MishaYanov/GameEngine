use serde::Serialize;
use std::process::Command;

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
