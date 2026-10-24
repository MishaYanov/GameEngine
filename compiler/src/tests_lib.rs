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
	prepared.validate_inputs().unwrap();
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
	prepared.validate_inputs().unwrap();
	fs::write(source.join("payload.bin"), b"modified").unwrap();
	let error = prepared.validate_inputs().unwrap_err();
	assert!(
		error.starts_with("Prepared asset \"payload.bin\" is no longer valid: "),
		"{error}"
	);
	assert!(error.contains("Asset changed after preflight"), "{error}");
	// Passing the early check must not bypass verification when copying later.
	let error = copy_prepared_assets(
		prepared.asset_root(),
		&fixture.0.join("copied"),
		&prepared.files,
	)
	.unwrap_err();
	assert!(error.to_string().contains("changed"), "{error}");
}

#[test]
fn prepared_input_validation_rejects_resized_missing_and_nonregular_assets_with_context() {
	for mutation in ["resized", "missing", "directory"] {
		let fixture = AssetFixture::new();
		let source = fixture.0.join("source");
		let (project, _) = authored_scene_fixture(&source);
		let relative = Path::new("nested").join("payload.bin");
		let input = source.join(&relative);
		fs::create_dir(source.join("nested")).unwrap();
		fs::write(&input, b"original").unwrap();
		let prepared = prepare_project(&project, &source).unwrap();
		prepared.validate_inputs().unwrap();
		match mutation {
			"resized" => fs::write(&input, b"longer replacement").unwrap(),
			"missing" => fs::remove_file(&input).unwrap(),
			"directory" => {
				fs::remove_file(&input).unwrap();
				fs::create_dir(&input).unwrap();
			}
			_ => unreachable!(),
		}
		let error = prepared.validate_inputs().unwrap_err();
		let context = format!("Prepared asset {relative:?} is no longer valid: ");
		let cause = error.strip_prefix(&context).expect(&error);
		assert!(!cause.is_empty(), "{error}");
		if mutation == "resized" {
			assert!(
				cause.contains("Asset changed size after preflight"),
				"{error}"
			);
		}
	}
}

#[test]
fn invalid_project_behavior_targets_and_missing_startup_scene_fail_before_a_build() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (mut project, _) = authored_scene_fixture(&source);
	project.add_behavior(BehaviorSpec::Spin {
		entity: engine_core::EntityKey::new("missing").unwrap(),
		angular_velocity: [0.0, 1.0, 0.0],
	});
	assert!(prepare_project(&project, &source).is_err());
	project.behaviors.clear();
	project
		.scenes
		.change_path(&project.startup_scene_id, "scenes/missing.json")
		.unwrap();
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

#[test]
fn project_preflight_checks_nonstartup_scene_files_models_and_scoped_behavior_targets() {
	use engine_core::{
		project::SceneBehavior,
		scene_catalog::{SceneDescriptor, SceneId},
	};

	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (mut project, mut secondary) = authored_scene_fixture(&source);
	let secondary_id = SceneId::new("secondary").unwrap();
	project
		.scenes
		.insert(SceneDescriptor::new(secondary_id.clone(), "scenes/secondary.json").unwrap())
		.unwrap();
	let context = "Scene \"secondary\" (\"scenes/secondary.json\"): ";
	let read_cause = read_authored_scene(&source, "scenes/secondary.json")
		.expect_err("secondary scene should be missing");
	let error = prepare_project(&project, &source)
		.err()
		.expect("missing nonstartup scene was accepted");
	assert!(error.starts_with(context), "{error}");
	assert!(error.ends_with(&read_cause), "{error}");

	fs::write(source.join("scenes/secondary.json"), "{").unwrap();
	let error = prepare_project(&project, &source)
		.err()
		.expect("malformed nonstartup scene was accepted");
	assert!(error.starts_with(context), "{error}");
	assert!(error.contains("Invalid scene JSON:"), "{error}");
	assert!(error.contains("EOF while parsing an object"), "{error}");

	secondary
		.entities
		.push(engine_core::scene_data::EntityData::new(
			engine_core::EntityKey::new("secondary-actor").unwrap(),
			"Secondary actor",
			"models/missing.glb",
		));
	fs::write(
		source.join("scenes/secondary.json"),
		secondary.to_json().unwrap(),
	)
	.unwrap();
	let error = prepare_project(&project, &source)
		.err()
		.expect("missing nonstartup model was accepted");
	assert!(error.starts_with(context), "{error}");
	assert!(
		error.contains("Model 'models/missing.glb' preflight failed:"),
		"{error}"
	);

	secondary.entities[0].renderable = false;
	fs::write(
		source.join("scenes/secondary.json"),
		secondary.to_json().unwrap(),
	)
	.unwrap();
	project.behaviors.push(SceneBehavior {
		scene_id: secondary_id,
		behavior: BehaviorSpec::Spin {
			entity: secondary.entities[0].key.clone(),
			angular_velocity: [0.0, 1.0, 0.0],
		},
	});
	let prepared = prepare_project(&project, &source).unwrap();
	let destination = fixture.0.join("copied");
	copy_prepared_assets(prepared.asset_root(), &destination, &prepared.files).unwrap();
	for scene in ["scenes/main.json", "scenes/secondary.json"] {
		assert_eq!(
			fs::read(source.join(scene)).unwrap(),
			fs::read(destination.join(scene)).unwrap()
		);
	}

	project.behaviors[0].behavior = BehaviorSpec::Spin {
		entity: engine_core::EntityKey::new("missing").unwrap(),
		angular_velocity: [0.0, 1.0, 0.0],
	};
	let error = prepare_project(&project, &source)
		.err()
		.expect("missing target in nonstartup scene was accepted");
	assert!(error.starts_with(context), "{error}");
	assert!(
		error.contains("Behavior 0 targets missing entity \"missing\""),
		"{error}"
	);
}

#[test]
fn preflight_reports_one_primary_failure_per_scene_in_stable_id_order() {
	use engine_core::{project::SceneBehavior, scene_catalog::SceneId};

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
	fs::write(source.join("scenes/alpha.json"), scene.to_json().unwrap()).unwrap();
	fs::write(source.join("scenes/zeta.json"), "{").unwrap();

	let reports = [["zeta", "alpha"], ["alpha", "zeta"]].map(|order| {
		let mut project = project.clone();
		for id in order {
			project
				.scenes
				.insert(
					SceneDescriptor::new(SceneId::new(id).unwrap(), format!("scenes/{id}.json"))
						.unwrap(),
				)
				.unwrap();
		}
		project.behaviors.push(SceneBehavior {
			scene_id: SceneId::new("alpha").unwrap(),
			behavior: BehaviorSpec::Spin {
				entity: engine_core::EntityKey::new("missing-target").unwrap(),
				angular_velocity: [0.0, 1.0, 0.0],
			},
		});
		prepare_project(&project, &source)
			.err()
			.expect("invalid catalog was accepted")
	});
	assert_eq!(reports[0], reports[1]);
	let lines: Vec<_> = reports[0].lines().collect();
	assert_eq!(lines.len(), 2, "{}", reports[0]);
	assert!(lines[0].starts_with("Scene \"alpha\" (\"scenes/alpha.json\"): "));
	assert!(lines[0].contains("Behavior 0 targets missing entity \"missing-target\""));
	assert!(lines[1].starts_with("Scene \"zeta\" (\"scenes/zeta.json\"): "));
	assert!(lines[1].contains("Invalid scene JSON:"));
	assert!(
		!reports[0].contains("models/missing.glb"),
		"a later cause hid the primary failure"
	);
}

#[test]
fn scene_report_cap_preserves_unicode_and_emits_one_final_notice() {
	let unicode_error = "🦀".repeat(16 * 1024);
	// Vary the existing content so truncation meets every UTF-8 byte offset.
	for prefix in ["", "x", "xx", "xxx"] {
		let mut report = String::new();
		let mut truncated = false;
		if !prefix.is_empty() {
			append_scene_error(&mut report, prefix, &mut truncated);
		}
		append_scene_error(&mut report, &unicode_error, &mut truncated);
		assert!(truncated);
		assert!(report.len() <= 16 * 1024);
		assert!(report.len() >= 16 * 1024 - 3);
		assert_eq!(report.matches(OMISSION_NOTICE).count(), 1);
		let content = report.strip_suffix(OMISSION_NOTICE).unwrap();
		let retained = if prefix.is_empty() {
			content
		} else {
			content.strip_prefix(&format!("{prefix}\n")).unwrap()
		};
		assert!(retained.chars().all(|character| character == '🦀'));
		let capped = report.clone();
		append_scene_error(&mut report, "later scene failure", &mut truncated);
		assert_eq!(report, capped);
	}
}

#[test]
fn copied_catalog_validation_uses_copied_scene_and_model_files() {
	use engine_core::scene_catalog::SceneId;

	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (mut project, mut scene) = authored_scene_fixture(&source);
	project
		.scenes
		.insert(
			SceneDescriptor::new(SceneId::new("secondary").unwrap(), "scenes/secondary.json")
				.unwrap(),
		)
		.unwrap();
	fs::write(
		source.join("scenes/secondary.json"),
		scene.to_json().unwrap(),
	)
	.unwrap();
	scene
		.entities
		.push(engine_core::scene_data::EntityData::new(
			engine_core::EntityKey::new("actor").unwrap(),
			"Actor",
			"models/actor.glb",
		));
	fs::create_dir(source.join("models")).unwrap();
	fs::write(source.join("models/actor.glb"), b"model fixture").unwrap();
	fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let prepared = prepare_project(&project, &source).unwrap();
	let copied = fixture.0.join("copied");
	copy_prepared_assets(prepared.asset_root(), &copied, &prepared.files).unwrap();
	let copied = copied.canonicalize().unwrap();
	validate_catalog_scenes(&project, &copied, &prepared.files).unwrap();

	fs::remove_file(copied.join("models/actor.glb")).unwrap();
	fs::write(copied.join("scenes/secondary.json"), "{").unwrap();
	let error = validate_catalog_scenes(&project, &copied, &prepared.files).unwrap_err();
	assert_eq!(error.lines().count(), 2, "{error}");
	assert!(
		error.contains("(\"scenes/main.json\"): Model 'models/actor.glb' preflight failed:"),
		"{error}"
	);
	assert!(
		error.contains("Scene \"secondary\" (\"scenes/secondary.json\"): Invalid scene JSON:"),
		"{error}"
	);
	// The authored source remains valid; these failures came from the copied tree.
	assert!(prepare_project(&project, &source).is_ok());
}

#[test]
fn catalog_validation_rejects_existing_scene_and_model_files_missing_from_the_snapshot() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (project, mut scene) = authored_scene_fixture(&source);
	scene
		.entities
		.push(engine_core::scene_data::EntityData::new(
			engine_core::EntityKey::new("actor").unwrap(),
			"Actor",
			"models/actor.glb",
		));
	fs::create_dir(source.join("models")).unwrap();
	fs::write(source.join("models/actor.glb"), b"model fixture").unwrap();
	fs::write(source.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let prepared = prepare_project(&project, &source).unwrap();
	let copied = fixture.0.join("copied");
	copy_prepared_assets(prepared.asset_root(), &copied, &prepared.files).unwrap();
	let copied = copied.canonicalize().unwrap();
	let context = format!(
		"Scene {:?} (\"scenes/main.json\"): ",
		project.startup_scene_id.as_str()
	);

	for omitted in ["scenes/main.json", "models/actor.glb"] {
		// Deterministically model a snapshot that did not capture this existing file.
		let mut stale_files = prepare_assets(prepared.asset_root()).unwrap();
		stale_files.retain(|asset| asset.relative != Path::new(omitted));
		assert_eq!(stale_files.len() + 1, prepared.files.len());
		for root in [prepared.asset_root(), copied.as_path()] {
			assert!(root.join(omitted).is_file());
			let error = validate_catalog_scenes(&project, root, &stale_files).unwrap_err();
			assert!(error.starts_with(&context), "{error}");
			assert!(
				error.contains("Asset is absent from the prepared asset snapshot"),
				"{error}"
			);
			if omitted == "models/actor.glb" {
				assert!(
					error.contains("Model 'models/actor.glb' preflight failed:"),
					"{error}"
				);
			}
			let fresh_files = prepare_assets(root).unwrap();
			validate_catalog_scenes(&project, root, &fresh_files).unwrap();
		}
	}
}

#[test]
fn prepared_asset_resolution_returns_a_canonical_file_and_rejects_directories() {
	let fixture = AssetFixture::new();
	let source = fixture.0.join("source");
	let (project, _) = authored_scene_fixture(&source);
	let prepared = prepare_project(&project, &source).unwrap();
	let packaged: HashSet<&Path> = prepared
		.files
		.iter()
		.map(|asset| asset.relative.as_path())
		.collect();
	let resolved = prepared_asset_path(
		prepared.asset_root(),
		Path::new("scenes/main.json"),
		&packaged,
	)
	.unwrap();
	assert_eq!(
		resolved,
		source.join("scenes/main.json").canonicalize().unwrap()
	);
	assert!(resolved.is_file());

	let directory = Path::new("scenes");
	let packaged_directory = HashSet::from([directory]);
	let error =
		prepared_asset_path(prepared.asset_root(), directory, &packaged_directory).unwrap_err();
	assert_eq!(error, "Asset must be a regular file");
}
