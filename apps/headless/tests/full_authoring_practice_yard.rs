use engine_core::{
	SceneData,
	asset_catalog::AssetId,
	command::{EditorCommandDto, EditorCommandRequest},
	editor::EditorSession,
	linked_prefab::resolve_scene,
	project::ProjectData,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
	process::Command,
};

const YARD: &str = "tests/fixtures/authoring-controller/full";
const SCENE: &str = "scenes/full-practice-yard.json";

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		let path = std::env::temp_dir().join(format!(
			"engine-full-authoring-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&path).unwrap();
		Self(path)
	}

	fn dir(&self, name: &str) -> PathBuf {
		let path = self.0.join(name);
		fs::create_dir(&path).unwrap();
		path
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, [u8; 32]> {
	fn add(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, [u8; 32]>) {
		for entry in fs::read_dir(directory).unwrap() {
			let entry = entry.unwrap();
			if entry.file_type().unwrap().is_dir() {
				add(root, &entry.path(), files);
			} else {
				let path = entry.path();
				files.insert(
					path.strip_prefix(root).unwrap().to_path_buf(),
					Sha256::digest(fs::read(path).unwrap()).into(),
				);
			}
		}
	}
	let mut files = BTreeMap::new();
	add(root, root, &mut files);
	files
}

fn replay(project: &Path, route: &Path, state_path: &Path, unrelated: &Path) -> Value {
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.current_dir(unrelated)
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(route)
		.arg("--state-output")
		.arg(state_path)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

fn onsets(report: &Value) -> Vec<&Value> {
	report["scene_audio_events"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|entry| entry["kind"] == "trigger")
		.map(|entry| &entry["onset"])
		.collect()
}

#[test]
fn practice_yard_owns_nested_props_and_replays_in_a_copied_package() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(YARD);
	let assets = root.join("assets");
	let project_path = root.join("project.json");
	let route_path = root.join("route.replay.json");
	let source_path = assets.join(SCENE);
	let source_bytes = fs::read(&source_path).unwrap();
	let source = SceneData::from_json(std::str::from_utf8(&source_bytes).unwrap()).unwrap();
	let project = ProjectData::load(&project_path).unwrap();

	assert_eq!(source.organization.folders[0].name, "Floor");
	let floor = &source.organization.folders[0].id;
	assert_eq!(
		source
			.organization
			.root_membership
			.values()
			.filter(|value| *value == floor)
			.count(),
		2
	);
	assert!(source.active_sky().unwrap().background.image().is_some());
	assert_eq!(source.environment.ambient_linear, [0.24, 0.29, 0.36]);
	assert_eq!(source.environment.sky_exposure_ev, 0.18);
	assert_eq!(
		source.default_game_camera.as_ref().unwrap().as_str(),
		"yard-camera"
	);
	let anchors = source
		.entities
		.iter()
		.filter_map(|entity| entity.linked_prefab.as_ref())
		.collect::<Vec<_>>();
	assert_eq!(anchors.len(), 2);
	assert!(!anchors[0].overrides.is_empty());
	assert!(anchors[1].overrides.is_empty());
	let resolved = resolve_scene(&source);
	for name in ["Left Bell", "Bell Body"] {
		assert!(
			resolved.entities.iter().any(|entity| entity.name == name),
			"{name}"
		);
	}
	let bell = resolved
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "active-bell")
		.unwrap();
	let visual = resolved
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "active-bell-visual")
		.unwrap();
	assert!(bell.collider.is_some());
	assert!(visual.collider.is_none());
	assert_eq!(visual.parent.as_ref(), Some(&bell.key));

	let mut editor = EditorSession::for_project(source.clone(), project.clone()).unwrap();
	let overridden = source
		.entities
		.iter()
		.find(|entity| entity.name == "Left Bell")
		.unwrap()
		.key
		.clone();
	let revision = editor.scene_revision();
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::Rename {
				key: overridden.as_str().into(),
				name: "Trial Bell".into(),
			},
		})
		.unwrap();
	assert_eq!(editor.scene_revision(), revision + 1);
	editor
		.execute_command(EditorCommandRequest {
			expected_revision: editor.revision(),
			command: EditorCommandDto::Undo,
		})
		.unwrap();
	assert_eq!(*editor.authored_document(), source);

	let scratch = Scratch::new();
	let out_a = scratch.dir("export-a");
	let out_b = scratch.dir("export-b");
	let foreign = scratch.dir("unrelated");
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, out_a.to_str().unwrap()).unwrap();
	let staged_again =
		compiler::stage_prepared_project(&prepared, out_b.to_str().unwrap()).unwrap();
	assert_eq!(inventory(staged.path()), inventory(staged_again.path()));
	assert!(compiler::stage_prepared_project(&prepared, out_a.to_str().unwrap()).is_err());
	assert_eq!(inventory(staged.path()), inventory(staged_again.path()));

	let source_state = scratch.0.join("saved-source.json");
	let package_state = scratch.0.join("saved-package.json");
	let authored = replay(&project_path, &route_path, &source_state, &foreign);
	let packaged = replay(
		&staged.path().join("project.json"),
		&route_path,
		&package_state,
		&foreign,
	);
	assert_eq!(authored["markers"], packaged["markers"]);
	assert_eq!(
		authored["accepted_action_presses"],
		packaged["accepted_action_presses"]
	);
	assert_eq!(onsets(&authored), onsets(&packaged));
	assert_eq!(onsets(&authored).len(), 1);
	assert_eq!(onsets(&authored)[0]["voice"]["entity"], "active-bell");
	assert_eq!(onsets(&authored)[0]["cause"]["marker"], "bell-contact");
	assert_eq!(onsets(&authored)[0]["voice"]["spatial"]["mode"], "spatial");
	assert_eq!(fs::read(source_path).unwrap(), source_bytes);
	assert_eq!(
		SceneData::from_json(&fs::read_to_string(source_state).unwrap()).unwrap(),
		source
	);
	assert!(
		staged
			.path()
			.join("assets/audio/practice-bell.wav")
			.is_file()
	);
	assert!(
		staged
			.path()
			.join("assets/animations/practice-bell.clip.json")
			.is_file()
	);

	let mut live_route: Value = serde_json::from_slice(&fs::read(route_path).unwrap()).unwrap();
	let actions = live_route["actions"].as_array_mut().unwrap();
	let swing_frame = actions
		.iter()
		.position(|action| action["label"] == "swing-pose")
		.unwrap();
	actions.truncate(swing_frame + 1);
	let live_path = scratch.0.join("live.replay.json");
	fs::write(&live_path, serde_json::to_vec(&live_route).unwrap()).unwrap();
	let live_source = replay(
		&project_path,
		&live_path,
		&scratch.0.join("live-source.json"),
		&foreign,
	);
	let live_export = replay(
		&staged.path().join("project.json"),
		&live_path,
		&scratch.0.join("live-package.json"),
		&foreign,
	);
	assert_eq!(live_source["graph_runtime"], live_export["graph_runtime"]);
	assert_eq!(
		live_source["camera_runtime"]["pose"],
		live_export["camera_runtime"]["pose"]
	);
	let graphs = &live_source["graph_runtime"];
	assert_eq!(graphs["control"]["actor"], "yard-player");
	let animator = graphs["animators"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| row["actor"] == "yard-player")
		.unwrap();
	assert_eq!(animator["state"], "swing");
	assert!(graphs["visual_world_matrices"]["active-bell-visual"].is_array());
}
