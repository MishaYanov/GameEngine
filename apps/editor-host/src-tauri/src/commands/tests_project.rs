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
