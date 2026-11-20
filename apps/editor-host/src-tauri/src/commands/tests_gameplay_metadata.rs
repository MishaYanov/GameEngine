use super::capabilities;

#[test]
fn advertised_modules_resolve_through_the_editors_gameplay_resolver() {
	let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/project.json");
	let mut project = runtime::project::ProjectData::load(manifest).unwrap();
	let capabilities = capabilities();
	assert!(!capabilities.gameplay_modules.is_empty());
	let mut ids = std::collections::HashSet::new();
	for module in &capabilities.gameplay_modules {
		assert!(ids.insert(&module.id), "Duplicate module ID");
		project.gameplay_module = Some(module.id.clone());
		assert!(courtyard_gameplay::resolve(&project).is_ok());
	}
	project.gameplay_module = None;
	assert!(courtyard_gameplay::resolve(&project).is_ok());
	let wire = serde_json::to_value(capabilities).unwrap();
	assert_eq!(
		wire["gameplay_modules"][0]["id"],
		courtyard_gameplay::COURTYARD_RUN_ID
	);
	assert_eq!(
		wire["gameplay_modules"][0]["scene_roles"]["finish"][1]["name"],
		"finish"
	);
}
