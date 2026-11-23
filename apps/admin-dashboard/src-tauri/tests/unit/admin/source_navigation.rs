use super::*;

#[test]
fn source_navigation_rejects_files_outside_workspace() {
	let fixture = Fixture::new();
	let workspace = fixture.0.join("workspace");
	fs::create_dir(&workspace).unwrap();
	fs::write(workspace.join("Cargo.toml"), "[workspace]\n").unwrap();
	fs::write(workspace.join("source.rs"), "// fixture").unwrap();
	fs::write(fixture.0.join("outside.rs"), "// outside").unwrap();
	let mut service = AdminService::new(workspace.clone(), fixture.logs(), fixture.data());
	let mut entry = record("game-123-1", 1);
	entry.file = Some("source.rs".into());
	entry.line = Some(4);
	service.collector.ingest(entry.clone());
	assert_eq!(service.source(1).unwrap().1, 4);
	entry.sequence = 2;
	entry.file = Some("../outside.rs".into());
	service.collector.ingest(entry);
	assert!(service.source(2).is_err());
	assert!(
		service
			.configure(Some(PathBuf::from("missing")), "logs")
			.is_err()
	);
}
