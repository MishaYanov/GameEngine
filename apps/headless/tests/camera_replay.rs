use engine_core::project::ProjectData;
use serde_json::{Value, json};
use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

const ROUTE: &str = include_str!("../../../docs/verification/milestone-10-camera-route.json");
const WALL: &str = include_str!("../../../docs/verification/milestone-10-camera-wall.json");

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		let path =
			std::env::temp_dir().join(format!("engine-camera-replay-{}", std::process::id()));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn run(project: &Path, file: &Path, actions: &[Value]) -> Value {
	fs::write(
		file,
		serde_json::to_vec(&json!({"schema_version":2,"actions":actions})).unwrap(),
	)
	.unwrap();
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.current_dir(file.parent().unwrap())
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(file)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_same_state(a: &Value, b: &Value) {
	for field in [
		"camera_runtime",
		"camera_runtime_hash",
		"native_components",
		"native_components_hash",
		"state_hash",
	] {
		assert_eq!(a[field], b[field], "{field}");
	}
}

#[test]
fn camera_room_sensor_doorway_lintel_and_rotated_proxy_match_the_authored_geometry() {
	use engine_core::{Camera, SceneData, camera_preview::FrozenOrbitPreview};
	use std::sync::Arc;
	let room =
		SceneData::from_json(include_str!("../../game/assets/scenes/camera-room.json")).unwrap();
	let canonical = room.to_json().unwrap();
	assert_eq!(
		SceneData::from_json(&canonical).unwrap().to_json().unwrap(),
		canonical
	);
	for (x, z, pitch, blocker) in [
		(0.0, 0.0, 0.0, None),
		(6.0, -7.0, 0.0, None),
		(5.0, -7.0, 0.0, Some("doorway-left")),
		(7.0, -7.0, 0.0, Some("doorway-right")),
		(6.0, -7.0, -0.5, Some("doorway-top")),
		(-5.0, -7.0, 0.0, Some("rotated-proxy")),
	] {
		let mut scene = room.clone();
		let mut rig = scene.camera.orbit.clone().unwrap();
		rig.initial_pitch = pitch;
		let target = scene
			.entities
			.iter_mut()
			.find(|e| e.key == rig.target)
			.unwrap();
		target.transform.position = [x, target.transform.position[1], z];
		let camera = Camera {
			position: scene.camera.position,
			yaw: scene.camera.yaw,
			pitch: scene.camera.pitch,
			vertical_fov: scene.camera.vertical_fov,
			near: scene.camera.near_plane,
			far: scene.camera.far_plane,
		};
		let preview =
			FrozenOrbitPreview::from_document(Arc::new(scene), rig, camera, 1.0 / 60.0).unwrap();
		let orbit = preview.orbit_state();
		assert_eq!(
			orbit.obstruction.as_ref().map(|hit| hit.blocker.as_str()),
			blocker,
			"at ({x}, {z}) pitch {pitch}"
		);
		assert_eq!(
			orbit.effective_distance < orbit.user_distance,
			blocker.is_some()
		);
	}
}

#[test]
fn camera_room_source_and_package_agree_at_each_checkpoint_and_across_tick_partitions() {
	let scratch = Scratch::new();
	let fixture = test_project::ProjectFixture::new("camera-room");
	let source = fixture.manifest_path();
	let project = ProjectData::load(&source).unwrap();
	let prepared =
		compiler::prepare_project(&project, &source.parent().unwrap().join("assets")).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, scratch.0.to_str().unwrap()).unwrap();
	let packaged = staged.path().join("project.json");
	let route: Value = serde_json::from_str(ROUTE).unwrap();
	let actions = route["actions"].as_array().unwrap();
	let wall_route: Value = serde_json::from_str(WALL).unwrap();
	assert_eq!(wall_route["actions"].as_array().unwrap(), &actions[..5]);
	let replay = scratch.0.join("route.json");
	let checkpoint = |count| {
		let a = run(&source, &replay, &actions[..count]);
		let b = run(&packaged, &replay, &actions[..count]);
		assert_same_state(&a, &b);
		a
	};

	let fresh = checkpoint(1);
	let wall = checkpoint(5);
	let moving_clear = checkpoint(7);
	let clear = checkpoint(9);
	let pending = checkpoint(10);
	let turned = checkpoint(11);
	let restarted = checkpoint(actions.len());

	let orbit = &wall["camera_runtime"]["orbit"];
	assert_eq!(orbit["target"]["key"], "player");
	assert_eq!(orbit["target"]["same_world"], true);
	assert_eq!(orbit["obstruction"]["blocker"], "camera-wall");
	assert_eq!(orbit["user_distance"], 5.0);
	let distance = orbit["effective_distance"].as_f64().unwrap();
	assert!(distance > 0.0 && distance < 4.0);
	let recovering = &moving_clear["camera_runtime"]["orbit"];
	assert!(recovering["obstruction"].is_null());
	assert_eq!(recovering["arm_settled"], false);
	let leaving_wall = recovering["effective_distance"].as_f64().unwrap();
	assert!(distance < leaving_wall && leaving_wall < 5.0);
	let recovered = &clear["camera_runtime"]["orbit"];
	assert!(recovered["obstruction"].is_null());
	assert_eq!(recovered["effective_distance"], 5.0);
	assert_eq!(recovered["arm_settled"], true);
	assert_ne!(pending["camera_runtime_hash"], clear["camera_runtime_hash"]);
	assert_eq!(
		pending["camera_runtime"]["pose"],
		clear["camera_runtime"]["pose"]
	);
	assert_eq!(
		pending["native_components_hash"],
		clear["native_components_hash"]
	);
	assert_eq!(pending["state_hash"], clear["state_hash"]);
	let after_input = &turned["camera_runtime"]["orbit"];
	assert_eq!(after_input["yaw"], 0.25);
	assert!((after_input["user_distance"].as_f64().unwrap() - 5.0 * (-0.1_f64).exp()).abs() < 1e-5);
	assert_same_state(&fresh, &restarted);

	let mut paused = actions[..10].to_vec();
	paused.push(json!({"type":"command","command":{"type":"set_paused","paused":true}}));
	paused.push(json!({"type":"tick","count":120}));
	let after_pause = run(&packaged, &replay, &paused);
	assert_eq!(after_pause["fixed_steps"], clear["fixed_steps"]);
	assert_same_state(&clear, &after_pause);

	let mut grouped = actions[..11].to_vec();
	for action in &mut grouped {
		if action["type"] == "tick" && action["count"].as_u64().unwrap() % 4 == 0 {
			action["count"] = json!(action["count"].as_u64().unwrap() / 4);
			action["elapsed_seconds"] = json!(4.0 / 60.0);
		}
	}
	let batched = run(&packaged, &replay, &grouped);
	assert_eq!(batched["fixed_steps"], turned["fixed_steps"]);
	assert_same_state(&turned, &batched);
}

#[path = "../../../tests/support/project_fixture.rs"]
mod test_project;
