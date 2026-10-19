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
mod tests {
    use super::*;

    #[test]
    fn invalid_project_request_returns_an_error_without_building() {
        let result =
            tauri::async_runtime::block_on(create_project("../escape".into(), "relative".into()));
        assert!(matches!(result, Err(message) if message.contains("project name")));
    }

    #[test]
    fn launch_failure_json_matches_the_frontend_contract() {
        let result = ProjectResult {
            path: "D:/Projects/Demo".into(),
            launched: false,
            launch_error: Some("executable missing".into()),
        };
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({
                "path": "D:/Projects/Demo", "launched": false, "launchError": "executable missing"
            })
        );
    }
}
