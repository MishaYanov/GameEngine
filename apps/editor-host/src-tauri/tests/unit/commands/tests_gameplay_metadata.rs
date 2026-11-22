use super::capabilities;

#[test]
fn advertised_modules_resolve_through_the_editors_gameplay_resolver() {
	let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../..")
		.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
		.join("project.json");
	let mut project = runtime::project::ProjectData::load(manifest).unwrap();
	let capabilities = capabilities();
	assert_eq!(capabilities.gameplay_modules.len(), 1);
	assert_eq!(
		project.gameplay_module.as_deref(),
		Some(gameplay_catalog::CINDER_OATH_ID)
	);
	assert!(gameplay_catalog::resolve(&project).is_ok());
	project.gameplay_module = None;
	assert!(gameplay_catalog::resolve(&project).is_ok());
	let wire = serde_json::to_value(capabilities).unwrap();
	assert_eq!(
		wire["state_machines"],
		serde_json::to_value(runtime::state_machine::descriptor()).unwrap()
	);
	assert_eq!(
		wire["gameplay_modules"][0]["id"],
		gameplay_catalog::CINDER_OATH_ID
	);
}
