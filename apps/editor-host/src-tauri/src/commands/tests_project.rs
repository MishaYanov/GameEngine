use super::*;

#[test]
fn invalid_project_request_returns_an_error_without_building() {
	let result = compiler::create_authoring_project("../escape", "relative");
	assert!(matches!(result, Err(message) if message.contains("project name")));
}

#[test]
fn authoring_project_json_matches_the_frontend_contract() {
	let result = ProjectResult {
		path: "D:/Projects/Demo".into(),
		manifest_path: "D:/Projects/Demo/project.json".into(),
	};
	assert_eq!(
		serde_json::to_value(result).unwrap(),
		serde_json::json!({
			"path": "D:/Projects/Demo", "manifestPath": "D:/Projects/Demo/project.json"
		})
	);
}
