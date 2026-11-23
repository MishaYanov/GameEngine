use engine_core::{SceneData, asset_catalog::AssetId, editor::PrefabData, project::ProjectData};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

const YARD: &str = "tests/fixtures/authoring-controller";
const ROUTE: &str = include_str!("fixtures/authoring-controller/graph-route.replay.json");
const SCENE: &str = "scenes/graph-practice-yard.json";
const CLIP: &str = "animations/yard-swing.clip.json";
const ANIMATOR: &str = "animations/yard.animator.json";
const CATALOG: &str = "asset-catalog.json";
const PROP: &str = "prefabs/training-blade.prefab.json";
const ASSET_FILES: &[&str] = &[
	SCENE,
	"scenes/practice-yard.json",
	CATALOG,
	"models/cube.glb",
	PROP,
	"controls/yard.input-actions.json",
	"controls/yard.controls.json",
	ANIMATOR,
	"animations/yard-idle.clip.json",
	"animations/yard-walk.clip.json",
	"animations/yard-jump.clip.json",
	CLIP,
];

struct Fixture(PathBuf);

struct Checkpoint {
	report: Value,
	document: SceneData,
}

impl Fixture {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!(
			"engine-authoring-graph-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn copy_project(&self, source: &Path) -> PathBuf {
		let path = self.0.join("authored");
		fs::create_dir(&path).unwrap();
		fs::copy(source.join("graph-project.json"), path.join("project.json")).unwrap();
		for file in ASSET_FILES {
			let relative = Path::new("assets").join(file);
			let destination = path.join(&relative);
			fs::create_dir_all(destination.parent().unwrap()).unwrap();
			fs::copy(source.join(relative), destination).unwrap();
		}
		path
	}

	fn replay(&self, manifest: &Path, actions: &[Value]) -> Checkpoint {
		let nonce = AssetId::generate();
		let route_path = self.0.join(format!("route-{}.json", nonce.as_str()));
		let state_path = self.0.join(format!("state-{}.json", nonce.as_str()));
		write_json(
			&route_path,
			&json!({"schema_version": 5, "actions": actions}),
		);
		let foreign = self.0.join("unrelated");
		fs::create_dir_all(&foreign).unwrap();
		let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
			.current_dir(foreign)
			.arg("--project")
			.arg(manifest)
			.arg("--replay")
			.arg(&route_path)
			.arg("--state-output")
			.arg(&state_path)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		Checkpoint {
			report: serde_json::from_slice(&output.stdout).unwrap(),
			document: SceneData::from_json(&fs::read_to_string(state_path).unwrap()).unwrap(),
		}
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Checkpoint {
	fn graph(&self) -> &Value {
		&self.report["graph_runtime"]
	}

	fn animator(&self) -> &Value {
		&self.graph()["animators"][0]
	}

	fn pose(&self) -> &Value {
		&self.graph()["visual_world_matrices"]["yard-training-blade"]
	}
}

#[test]
fn keyboard_actions_animator_transitions_poses_and_camera_match_staged_play() {
	let fixture = Fixture::new();
	let authored = fixture.copy_project(&fixture_source());
	let source = authored.join("project.json");
	let root = authored.join("assets");
	let project = ProjectData::load(&source).unwrap();
	let prepared = compiler::prepare_project(&project, &root).unwrap();
	let export = fixture.0.join("export");
	fs::create_dir(&export).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, export.to_str().unwrap()).unwrap();
	let packaged = staged.path().join("project.json");

	for file in ASSET_FILES
		.iter()
		.filter(|file| ![CATALOG, SCENE, PROP, "scenes/practice-yard.json"].contains(file))
	{
		assert_eq!(
			fs::read(root.join(file)).unwrap(),
			fs::read(staged.path().join("assets").join(file)).unwrap(),
			"{file}"
		);
	}
	let read_prop = |asset_root: &Path| {
		PrefabData::from_json(&fs::read_to_string(asset_root.join(PROP)).unwrap()).unwrap()
	};
	assert_eq!(read_prop(&root), read_prop(&staged.path().join("assets")));
	let route: Value = serde_json::from_str(ROUTE).unwrap();
	let actions = route["actions"].as_array().unwrap();
	let checkpoint = |name: &str| {
		let stop = actions
			.iter()
			.position(|action| action["label"] == name)
			.unwrap();
		let original = fixture.replay(&source, &actions[..=stop]);
		let exported = fixture.replay(&packaged, &actions[..=stop]);
		assert_eq!(original.graph(), exported.graph(), "{name}");
		for field in [
			"accepted_action_presses",
			"camera_runtime",
			"native_components",
			"native_components_hash",
			"state_hash",
		] {
			assert_eq!(
				original.report[field], exported.report[field],
				"{name}: {field}"
			);
		}
		assert_eq!(original.document, exported.document, "{name}");
		assert_eq!(
			original.report["native_components_hash_scope"],
			"native_components_v1"
		);
		assert_eq!(original.graph()["schema_version"], 1);
		assert_eq!(original.animator()["actor"], "yard-player");
		assert_eq!(original.graph()["control"]["actor"], "yard-player");
		assert_eq!(original.pose().as_array().unwrap().len(), 16);
		original
	};

	let idle = checkpoint("idle");
	assert_eq!(idle.animator()["state"], "idle");
	let walk_enter = checkpoint("walk-enter");
	let walk = checkpoint("walk");
	assert_eq!(walk.animator()["state"], "walk");
	assert_eq!(walk_enter.animator()["transition"], "idle_to_walk");
	assert_eq!(
		walk.graph()["control"]["output"]["intent"]["movement"],
		json!([1.0, 0.0])
	);
	assert_ne!(walk.pose(), idle.pose());

	let jump = checkpoint("jump");
	assert_eq!(jump.animator()["state"], "jump");
	assert_eq!(jump.animator()["transition"], "walk_to_jump");
	assert_eq!(
		jump.graph()["control"]["output"]["intent"]["jump"]["pressed"],
		true
	);
	assert_eq!(
		jump.report["native_components"]["characters"]["states"]["yard-player"]["grounded"],
		false
	);
	assert_eq!(accepted(&jump, "jump"), 1);

	let landed = checkpoint("landed");
	assert_eq!(landed.animator()["state"], "idle");
	assert_eq!(
		landed.report["native_components"]["characters"]["states"]["yard-player"]["grounded"],
		true
	);

	let swing = checkpoint("accepted-swing");
	assert_eq!(swing.animator()["state"], "swing");
	assert_eq!(swing.animator()["transition"], "idle_to_swing");
	assert_eq!(
		swing.graph()["control"]["output"]["intent"]["attack"]["pressed"],
		true
	);
	assert_eq!(
		swing.graph()["control"]["output"]["presentation"]["aim"],
		true
	);
	assert_eq!(accepted(&swing, "attack"), 1);
	let pose = checkpoint("swing-pose");
	assert_eq!(
		pose.graph()["control"]["output"]["intent"]["attack"]["pressed"],
		false
	);
	assert_eq!(accepted(&pose, "attack"), 1);
	assert_ne!(pose.pose(), swing.pose());

	let recovered = checkpoint("recovered");
	assert_eq!(recovered.animator()["state"], "idle");
	let restarted = checkpoint("restarted");
	assert_eq!(restarted.animator(), idle.animator());
	assert_eq!(restarted.pose(), idle.pose());
	assert_eq!(
		restarted.report["camera_runtime"]["pose"],
		idle.report["camera_runtime"]["pose"]
	);

	let swing_end = actions
		.iter()
		.position(|action| action["label"] == "swing-pose")
		.unwrap();
	let gamepad: Vec<_> = actions[..=swing_end]
		.iter()
		.map(|action| {
			if action["type"] != "keyboard_input" {
				return action.clone();
			}
			let keys = action["keys"].as_array().unwrap();
			let held = |key: &str| keys.iter().any(|value| value == key);
			json!({
				"type": "physical_controller_input",
				"left_x": if held("ArrowRight") { i16::MAX } else { 0 },
				"left_y": 0, "right_x": 0, "right_y": 0,
				"face_buttons": [held("KeyJ"), false, held("KeyF"), false]
			})
		})
		.collect();
	let from_gamepad = fixture.replay(&packaged, &gamepad);
	assert_eq!(from_gamepad.graph(), pose.graph());
	assert_eq!(from_gamepad.document, pose.document);
	assert_eq!(accepted(&from_gamepad, "attack"), 1);

	let swing_step = actions
		.iter()
		.position(|action| action["label"] == "accepted-swing")
		.unwrap();
	let mut manifest = read_json(&source);
	manifest["behaviors"] = json!([]);
	write_json(&source, &manifest);
	let request_only = fixture.replay(&source, &actions[..=swing_step]);
	assert_eq!(
		request_only.graph()["control"]["output"]["intent"]["attack"]["pressed"],
		true
	);
	assert_eq!(accepted(&request_only, "attack"), 0);
	assert_eq!(request_only.animator()["state"], "idle");

	let stopped = fixture.replay(&packaged, actions);
	assert!(stopped.graph().is_null());
	assert_eq!(
		stopped.document,
		SceneData::from_json(&fs::read_to_string(root.join(SCENE)).unwrap()).unwrap()
	);
}

#[test]
fn a_missing_or_wrong_clip_stops_preflight_before_the_destination_exists() {
	let fixture = Fixture::new();
	let project_path = fixture.copy_project(&fixture_source());
	let project = ProjectData::load(&project_path.join("project.json")).unwrap();
	let assets = project_path.join("assets");
	let export = fixture.0.join("export");
	fs::create_dir(&export).unwrap();
	let clip = assets.join(CLIP);
	let saved_clip = fs::read(&clip).unwrap();
	fs::remove_file(&clip).unwrap();
	let result = compiler::prepare_project(&project, &assets)
		.and_then(|prepared| compiler::stage_prepared_project(&prepared, export.to_str().unwrap()));
	let error = result.err().expect("missing clip must fail admission");
	assert!(
		error.contains("yard-swing") && error.contains("clip"),
		"{error}"
	);
	assert_eq!(fs::read_dir(&export).unwrap().count(), 0);
	fs::write(&clip, &saved_clip).unwrap();

	let animator_path = assets.join(ANIMATOR);
	let mut animator: Value = read_json(&animator_path);
	animator["states"][3]["clip"]["expected_sha256"] = json!("0".repeat(64));
	write_json(&animator_path, &animator);
	let mutated = fs::read(&animator_path).unwrap();
	let catalog_path = assets.join(CATALOG);
	let mut catalog = read_json(&catalog_path);
	let asset = catalog["assets"]
		.as_array_mut()
		.unwrap()
		.iter_mut()
		.find(|asset| asset["path"] == ANIMATOR)
		.unwrap();
	asset["bytes"] = json!(mutated.len());
	asset["fingerprint"] = json!(format!("{:x}", Sha256::digest(&mutated)));
	write_json(&catalog_path, &catalog);
	let result = compiler::prepare_project(&project, &assets)
		.and_then(|prepared| compiler::stage_prepared_project(&prepared, export.to_str().unwrap()));
	let error = result.err().expect("wrong clip pin must fail admission");
	assert!(
		error.contains("swing") && error.contains("changed clip revision"),
		"{error}"
	);
	assert_eq!(fs::read_dir(&export).unwrap().count(), 0);
	assert_eq!(fs::read(clip).unwrap(), saved_clip);
}

fn accepted(checkpoint: &Checkpoint, action: &str) -> usize {
	checkpoint.report["accepted_action_presses"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|press| press["id"] == action)
		.count()
}

fn fixture_source() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join(YARD)
}

fn read_json(path: &Path) -> Value {
	serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write_json(path: &Path, value: &Value) {
	fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
