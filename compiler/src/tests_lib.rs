use super::*;
#[test]
fn project_names_enforce_length_and_normalize_surrounding_whitespace() {
	let root = env!("CARGO_MANIFEST_DIR");
	assert!(project_path(&"a".repeat(81), root).is_err());
	assert!(project_path(&"a".repeat(80), root).is_ok());
	assert_eq!(
		project_path("  Demo QA  ", root).unwrap(),
		Path::new(root).join("Demo QA")
	);
	assert!(project_path("Demo", "").is_err());
}
#[test]
fn installation_copies_models_and_nested_dependencies() {
	let destination = std::env::temp_dir().join(format!("engine-assets-{}", std::process::id()));
	fs::create_dir(&destination).unwrap();
	let source = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.unwrap()
		.join("apps/game/assets");
	let result = copy_assets(&source, &destination);
	let model = fs::read(destination.join("props/robot.glb"));
	let texture = fs::read(destination.join("textures/crate.png"));
	let scene = fs::read(destination.join("scenes/robots.json"));
	fs::remove_dir_all(&destination).unwrap();
	result.unwrap();
	assert_eq!(
		model.unwrap(),
		fs::read(source.join("props/robot.glb")).unwrap()
	);
	assert_eq!(
		texture.unwrap(),
		fs::read(source.join("textures/crate.png")).unwrap()
	);
	assert_eq!(
		scene.unwrap(),
		fs::read(source.join("scenes/robots.json")).unwrap()
	);
}
#[test]
fn rejects_unsafe_names_and_destinations() {
	for name in ["", "../escape", "a/b", "CON", "Lpt1", "a:stream"] {
		assert!(project_path(name, env!("CARGO_MANIFEST_DIR")).is_err());
	}
	assert!(project_path("Demo", "relative").is_err());
}
#[test]
fn resolves_new_project_and_rejects_existing_folder() {
	let root = env!("CARGO_MANIFEST_DIR");
	assert_eq!(
		project_path("Triangle Demo", root).unwrap(),
		Path::new(root).join("Triangle Demo")
	);
	assert!(project_path("src", root).is_err());
}

struct AssetFixture(PathBuf);
impl AssetFixture {
	fn new() -> Self {
		static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
		let root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!(
				"qa-package-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		fs::create_dir(root.join("source")).unwrap();
		Self(root)
	}
}
impl Drop for AssetFixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn packaging_preserves_scene_and_prefab_json_bytes_in_nested_asset_directories() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	fs::create_dir(source.join("prefabs")).unwrap();
	let scene = br#"{"schema_version":3,"entities":[],"lights":[]}"#;
	let prefab = br#"{"schema_version":1,"entities":[{"name":"Robot"}]}"#;
	fs::write(source.join("scene.json"), scene).unwrap();
	fs::write(source.join("prefabs/robot.JSON"), prefab).unwrap();
	let destination = fixture.0.join("installed");
	copy_assets(&source, &destination).unwrap();
	assert_eq!(fs::read(destination.join("scene.json")).unwrap(), scene);
	assert_eq!(
		fs::read(destination.join("prefabs/robot.JSON")).unwrap(),
		prefab
	);
}

#[test]
fn unsupported_packaged_asset_is_rejected_before_creating_destination() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	fs::write(source.join("valid.json"), "{}").unwrap();
	fs::write(source.join("program.exe"), "fixture").unwrap();
	let destination = fixture.0.join("installed");
	assert!(copy_assets(&source, &destination).is_err());
	assert!(!destination.exists());
}

#[test]
fn oversized_packaged_json_is_rejected_from_metadata_without_reading_or_copying_it() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let file = fs::File::create(source.join("oversized.json")).unwrap();
	file.set_len(16 * 1024 * 1024 + 1).unwrap();
	drop(file);
	let destination = fixture.0.join("installed");
	let error = copy_assets(&source, &destination).unwrap_err();
	assert!(error.to_string().contains("16 MiB"));
	assert!(!destination.exists());
}

fn authored_scene_fixture(source: &Path) -> (ProjectData, SceneData) {
	fs::create_dir_all(source.join("scenes")).unwrap();
	let camera = engine_core::Camera::default();
	let scene = SceneData {
		schema_version: engine_core::scene_data::CURRENT_SCHEMA_VERSION,
		entities: vec![],
		bindings: Default::default(),
		lights: vec![],
		camera: engine_core::scene_data::CameraData {
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
	};
	fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let project = ProjectData::for_scene("Authored project", "scenes/main.json", &scene).unwrap();
	(project, scene)
}

#[test]
fn preparing_an_authored_project_captures_its_actual_scene_assets_and_behavior_config() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (project, _) = authored_scene_fixture(&source);
	fs::create_dir(source.join("prefabs")).unwrap();
	fs::write(source.join("prefabs/one.json"), "{}").unwrap();
	let prepared = prepare_project(&project, &source).unwrap();
	assert_eq!(prepared.project(), &project);
	assert_eq!(prepared.asset_root(), source.canonicalize().unwrap());
	assert_eq!(prepared.asset_count(), 2);
	assert!(prepared.total_bytes() > 0);
	let destination = fixture.0.join("copied");
	copy_prepared_assets(prepared.asset_root(), &destination, &prepared.files).unwrap();
	assert_eq!(
		fs::read(source.join("scenes/main.json")).unwrap(),
		fs::read(destination.join("scenes/main.json")).unwrap()
	);
}

#[test]
fn changed_assets_after_project_preparation_are_rejected_even_when_byte_length_matches() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (project, _) = authored_scene_fixture(&source);
	fs::write(source.join("payload.bin"), b"original").unwrap();
	let prepared = prepare_project(&project, &source).unwrap();
	fs::write(source.join("payload.bin"), b"modified").unwrap();
	let error = copy_prepared_assets(
		prepared.asset_root(),
		&fixture.0.join("copied"),
		&prepared.files,
	)
	.unwrap_err();
	assert!(error.to_string().contains("changed"), "{error}");
}

#[test]
fn invalid_project_behavior_targets_and_missing_startup_scene_fail_before_a_build() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (mut project, _) = authored_scene_fixture(&source);
	project.behaviors = vec![BehaviorSpec::Spin {
		entity: engine_core::EntityKey::new("missing").unwrap(),
		angular_velocity: [0.0, 1.0, 0.0],
	}];
	assert!(prepare_project(&project, &source).is_err());
	project.behaviors.clear();
	project.startup_scene = "scenes/missing.json".into();
	assert!(prepare_project(&project, &source).is_err());
}

#[test]
fn project_preflight_rejects_missing_renderable_models_but_allows_transform_only_groups() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (project, mut scene) = authored_scene_fixture(&source);
	scene
		.entities
		.push(engine_core::scene_data::EntityData::new(
			engine_core::EntityKey::new("actor").unwrap(),
			"Actor",
			"models/missing.glb",
		));
	fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	assert!(
		prepare_project(&project, &source).is_err(),
		"a derived game must not be packaged without its referenced model"
	);
	scene.entities[0].renderable = false;
	fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	assert!(prepare_project(&project, &source).is_ok());
}
