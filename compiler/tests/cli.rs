use engine_core::{project::ProjectData, scene_catalog::SceneId, scene_data::SceneData};
use std::{
	fs,
	path::PathBuf,
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../target")
			.join(format!(
				"qa-compiler-cli-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		Self(root)
	}
	fn command(&self) -> Command {
		let mut command = Command::new(env!("CARGO_BIN_EXE_compiler"));
		command.current_dir(&self.0);
		command
	}
}
impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn stage_cli_exports_authored_inputs_without_a_native_build_and_never_overwrites() {
	let fixture = Fixture::new();
	let authoring = fixture.0.join("authoring");
	let destination = fixture.0.join("exports");
	fs::create_dir_all(authoring.join("assets/scenes")).unwrap();
	fs::create_dir(&destination).unwrap();
	let scene = SceneData::from_json(
		r#"{
		"schema_version":3,"bindings":{},"entities":[],"lights":[],
		"camera":{"position":[0,0,5],"yaw":0,"pitch":0,"vertical_fov":1,"near":0.1,"far":100}
	}"#,
	)
	.unwrap();
	let project = ProjectData::for_scene_with_id(
		"CPU stage",
		SceneId::new("main").unwrap(),
		"scenes/main.json",
		&scene,
	)
	.unwrap();
	let manifest = authoring.join("project.json");
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	let scene_bytes = scene.to_json().unwrap();
	fs::write(authoring.join("assets/scenes/main.json"), &scene_bytes).unwrap();
	let invoke = || {
		fixture
			.command()
			.arg("--stage")
			.arg(&manifest)
			.arg(&destination)
			.output()
			.unwrap()
	};
	let staged = invoke();
	assert!(
		staged.status.success(),
		"{}",
		String::from_utf8_lossy(&staged.stderr)
	);
	let exported = destination.join("CPU stage");
	assert!(String::from_utf8_lossy(&staged.stdout).contains("Project staged at"));
	assert_eq!(
		ProjectData::load(exported.join("project.json")).unwrap(),
		project
	);
	assert_eq!(
		fs::read_to_string(exported.join("assets/scenes/main.json")).unwrap(),
		scene_bytes
	);
	assert!(
		!exported
			.join(format!("game{}", std::env::consts::EXE_SUFFIX))
			.exists()
	);
	let manifest_bytes = fs::read(exported.join("project.json")).unwrap();
	let rejected = invoke();
	assert!(!rejected.status.success());
	assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
	assert_eq!(
		fs::read(exported.join("project.json")).unwrap(),
		manifest_bytes
	);
	let mut unknown = project;
	unknown.gameplay_module = Some("unavailable".into());
	unknown.name = "Rejected module".into();
	fs::write(&manifest, unknown.to_json().unwrap()).unwrap();
	let rejected = invoke();
	assert!(!rejected.status.success());
	assert!(String::from_utf8_lossy(&rejected.stderr).contains("unavailable"));
	assert!(!destination.join("Rejected module").exists());
}

#[test]
fn stage_cli_rejects_incomplete_or_unknown_argument_forms() {
	let fixture = Fixture::new();
	for args in [
		vec![],
		vec!["--stage"],
		vec!["--stage", "project.json"],
		vec!["--export"],
		vec!["--export", "project.json"],
		vec!["--unknown", "destination"],
		vec!["Name", "destination", "extra"],
	] {
		let output = fixture.command().args(args).output().unwrap();
		assert_eq!(output.status.code(), Some(2));
		assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: compiler"));
	}
}

#[test]
fn export_cli_checks_manifest_before_creating_an_output_folder_or_building_player() {
	let fixture = Fixture::new();
	let output = fixture
		.command()
		.arg("--export")
		.arg("missing.json")
		.arg(&fixture.0)
		.output()
		.unwrap();
	assert_eq!(output.status.code(), Some(1));
	assert!(!String::from_utf8_lossy(&output.stderr).contains("Usage: compiler"));
	assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn legacy_names_starting_with_a_hyphen_reach_project_validation() {
	let fixture = Fixture::new();
	let output = fixture
		.command()
		.arg("-Game")
		.arg(fixture.0.join("missing-destination"))
		.output()
		.unwrap();
	assert_eq!(output.status.code(), Some(1));
	let error = String::from_utf8_lossy(&output.stderr);
	assert!(error.contains("Destination must be an existing absolute folder path"));
	assert!(!error.contains("Usage: compiler"));
}

#[test]
fn audio_stage_preserves_only_referenced_bytes_and_rejects_malformed_clips() {
	const KEY_CLIP: &[u8] =
		include_bytes!("../../apps/game/fixtures/courtyard-audio/assets/audio/cue-key.wav");
	let fixture = Fixture::new();
	let authoring = fixture.0.join("audio-authoring");
	let assets = authoring.join("assets");
	let audio = assets.join("audio");
	let destination = fixture.0.join("audio-exports");
	fs::create_dir_all(assets.join("scenes")).unwrap();
	fs::create_dir(&audio).unwrap();
	fs::create_dir(&destination).unwrap();
	let source = r#"{
		"schema_version":3,"bindings":{},"entities":[],"lights":[],
		"camera":{"position":[0,0,5],"yaw":0,"pitch":0,"vertical_fov":1,"near":0.1,"far":100}
	}"#;
	let scene = SceneData::from_json(source).unwrap();
	fs::write(assets.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
	let mut project =
		ProjectData::for_scene("CPU audio stage", "scenes/main.json", &scene).unwrap();
	let manifest = authoring.join("project.json");
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	use bridge::feedback_audio::{CueAssignment, FeedbackCue, prepare_catalog_with_mix};
	fs::write(audio.join("cue-key.wav"), KEY_CLIP).unwrap();
	fs::write(audio.join("cue-unused.wav"), KEY_CLIP).unwrap();
	let mix = serde_json::from_value(serde_json::json!({
		"bus_percent": {"collection": 35},
		"scene_bus_percent": { (project.startup_scene_id.as_str()): {"mechanisms": 60} },
	}))
	.unwrap();
	let catalog = prepare_catalog_with_mix(
		&assets,
		vec![
			CueAssignment {
				event: FeedbackCue::KeyCollected,
				path: "audio/cue-key.wav".into(),
			},
			CueAssignment {
				event: FeedbackCue::GateOpened,
				path: "audio/cue-key.wav".into(),
			},
		],
		mix,
	)
	.unwrap();
	fs::write(audio.join("cues.json"), catalog.manifest()).unwrap();

	let stage = || {
		fixture
			.command()
			.arg("--stage")
			.arg(&manifest)
			.arg(&destination)
			.output()
			.unwrap()
	};
	let success = stage();
	assert!(
		success.status.success(),
		"{}",
		String::from_utf8_lossy(&success.stderr)
	);
	let staged_audio = destination.join("CPU audio stage/assets/audio");
	assert_eq!(
		fs::read(staged_audio.join("cues.json")).unwrap(),
		catalog.manifest()
	);
	assert_eq!(
		fs::read(staged_audio.join("cue-key.wav")).unwrap(),
		KEY_CLIP
	);
	assert_eq!(fs::read_dir(&staged_audio).unwrap().count(), 2);

	project.name = "Rejected audio stage".into();
	fs::write(&manifest, project.to_json().unwrap()).unwrap();
	fs::write(
		audio.join("cues.json"),
		br#"{"schema_version":2,"cues":[],"scene_bus_percent":{"unregistered":{}}}"#,
	)
	.unwrap();
	let unknown = stage();
	assert!(!unknown.status.success());
	assert!(String::from_utf8_lossy(&unknown.stderr).contains("unregistered scene"));
	assert!(!destination.join("Rejected audio stage").exists());

	fs::write(audio.join("cues.json"), catalog.manifest()).unwrap();
	let mut corrupt = KEY_CLIP.to_vec();
	corrupt[24] = 0;
	fs::write(audio.join("cue-key.wav"), corrupt).unwrap();
	let invalid = stage();
	assert!(!invalid.status.success());
	assert!(String::from_utf8_lossy(&invalid.stderr).contains("canonical 16 kHz"));
	assert!(!destination.join("Rejected audio stage").exists());

	fs::remove_file(audio.join("cue-key.wav")).unwrap();
	let missing = stage();
	assert!(!missing.status.success());
	assert!(!destination.join("Rejected audio stage").exists());
}

#[test]
fn campaign_diagnostics_name_the_same_garden_role_as_editor_preparation() {
	use engine_core::scene_data::ColliderData;
	let fixture = Fixture::new();
	let authoring = fixture.0.join("campaign");
	let assets = authoring.join("assets");
	let output = fixture.0.join("exports");
	fs::create_dir_all(assets.join("scenes")).unwrap();
	fs::create_dir(assets.join("props")).unwrap();
	fs::create_dir(&output).unwrap();
	let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../apps/game");
	let manifest = authoring.join("project.json");
	fs::copy(source.join("project.courtyard-chapters.json"), &manifest).unwrap();
	let project = ProjectData::load(&manifest).unwrap();
	for scene in project.scenes.iter() {
		fs::copy(
			source.join("assets").join(scene.asset_path()),
			assets.join(scene.asset_path()),
		)
		.unwrap();
	}
	for model in ["capsule.glb", "cube.glb", "sphere.glb"] {
		fs::copy(
			source.join("assets/props").join(model),
			assets.join("props").join(model),
		)
		.unwrap();
	}
	let path = assets.join("scenes/chapter-garden.json");
	let original = fs::read_to_string(&path).unwrap();
	let mut garden = SceneData::from_json(&original).unwrap();
	let trigger = garden
		.entities
		.iter_mut()
		.find(|entity| entity.key.as_str() == "garden-exit-trigger")
		.unwrap();
	match trigger.collider.as_mut().unwrap() {
		ColliderData::Box { interaction, .. } | ColliderData::Sphere { interaction, .. } => {
			interaction.sensor = false;
		}
	}
	fs::write(&path, garden.to_json().unwrap()).unwrap();
	let stage = || {
		fixture
			.command()
			.arg("--stage")
			.arg(&manifest)
			.arg(&output)
			.output()
			.unwrap()
	};
	let rejected = stage();
	assert!(!rejected.status.success());
	let os_error = String::from_utf8_lossy(&rejected.stderr);
	assert!(
		os_error.contains(
			r#"Scene "garden" ("scenes/chapter-garden.json"): Exit trigger needs an enabled sensor collider"#
		),
		"{os_error}"
	);
	assert!(fs::read_dir(&output).unwrap().next().is_none());

	fs::write(&path, original).unwrap();
	let accepted = stage();
	assert!(
		accepted.status.success(),
		"{}",
		String::from_utf8_lossy(&accepted.stderr)
	);
	assert!(output.join("Courtyard Chapters/project.json").is_file());
}
