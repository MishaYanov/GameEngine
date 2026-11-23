use super::*;
use crate::workspace_config::{WorkspaceSettings, resolve_workspace};

#[test]
fn saved_checkout_requires_matching_explicit_trust() {
	let fixture = Fixture::new();
	let workspace = fixture.0.join("checkout");
	let other = fixture.0.join("other");
	for path in [&workspace, &other] {
		fs::create_dir(path).unwrap();
		fs::write(path.join("Cargo.toml"), "[workspace]\n").unwrap();
	}
	let workspace = resolve_workspace(&workspace).unwrap();
	let other = resolve_workspace(&other).unwrap();
	let logs = fixture.logs();
	let mut service = AdminService::new(workspace.clone(), logs.clone(), fixture.data());
	let legacy: WorkspaceSettings = serde_json::from_value(serde_json::json!({
		"workspace": workspace, "logs": logs
	}))
	.unwrap();
	assert!(!legacy.trusts(&workspace));
	assert!(!service.snapshot().workspace_trusted);
	assert!(service.trusted_workspace().is_err());

	service
		.configure(Some(workspace.clone()), logs.to_str().unwrap())
		.unwrap();
	assert_eq!(service.trusted_workspace().unwrap(), workspace);
	let settings_path = service.data.join("settings.json");
	let settings: WorkspaceSettings =
		serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
	assert!(settings.trusts(&workspace));
	assert!(!settings.trusts(&other));

	service.configure(None, logs.to_str().unwrap()).unwrap();
	let cleared: WorkspaceSettings =
		serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
	assert!(!cleared.trusts(&workspace));
	assert!(service.trusted_workspace().is_err());
}
