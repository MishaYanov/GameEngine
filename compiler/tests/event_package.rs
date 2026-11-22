use engine_core::{
	project::ProjectData, scene_data::SceneData, simulation_session::SimulationSession,
};
use std::{fs, path::PathBuf, process::Command, sync::Arc};
#[test]
fn staged_event_scene_preserves_machine_definitions_and_playable_outcome() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
	let path = root
		.join("target")
		.join(format!("qa-event-package-{}", std::process::id()));
	fs::create_dir(&path).unwrap();
	struct Cleanup(PathBuf);
	impl Drop for Cleanup {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}
	let _cleanup = Cleanup(path.clone());
	let project = test_project::ProjectFixture::new("event-lab");
	let output = Command::new(env!("CARGO_BIN_EXE_compiler"))
		.arg("--stage")
		.arg(project.manifest_path())
		.arg(&path)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let installed = path.join("Event Interaction Lab");
	let project = ProjectData::load(installed.join("project.json")).unwrap();
	let scene = SceneData::from_json(
		&fs::read_to_string(installed.join("assets/scenes/event-lab.json")).unwrap(),
	)
	.unwrap();
	let source = SceneData::from_json(
		&fs::read_to_string(root.join("apps/game/assets/scenes/event-lab.json")).unwrap(),
	)
	.unwrap();
	assert_eq!(scene, source);
	assert!(installed.join("assets/props/robot.glb").is_file());
	let mut session = SimulationSession::new(project, Arc::new(scene)).unwrap();
	session.set_controlled_input([0., 0., -1.]).unwrap();
	for _ in 0..180 {
		session.tick(1. / 60.).unwrap();
	}
	let inspection = session.runtime_inspection("test", "test", 0, 128).unwrap();
	let completed = inspection
		.entities
		.iter()
		.find(|e| e.handle.key.as_str() == "a-complete")
		.unwrap();
	assert_eq!(
		completed.behavior_states[0].machine_state.as_deref(),
		Some("done")
	);
	assert_eq!(inspection.event_sequence, 6);
}

#[path = "../../tests/support/project_fixture.rs"]
mod test_project;
