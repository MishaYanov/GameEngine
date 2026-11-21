use engine_core::{
	project::ProjectData, scene_data::SceneData, simulation_session::SimulationSession,
};
use std::{fs, path::PathBuf, process::Command, sync::Arc};

#[test]
fn staged_character_room_preserves_settings_and_source_motion() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
	let path = root
		.join("target")
		.join(format!("qa-character-package-{}", std::process::id()));
	fs::create_dir(&path).unwrap();
	struct Cleanup(PathBuf);
	impl Drop for Cleanup {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}
	let _cleanup = Cleanup(path.clone());
	let source_project = ProjectData::load(root.join("apps/game/controller-demo.json")).unwrap();
	let source_scene = SceneData::from_json(
		&fs::read_to_string(root.join("apps/game/assets/scenes/controller-room.json")).unwrap(),
	)
	.unwrap();
	let output = Command::new(env!("CARGO_BIN_EXE_compiler"))
		.arg("--stage")
		.arg(root.join("apps/game/controller-demo.json"))
		.arg(&path)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let installed = path.join("Character Controller Room");
	let project = ProjectData::load(installed.join("project.json")).unwrap();
	let scene = SceneData::from_json(
		&fs::read_to_string(installed.join("assets/scenes/controller-room.json")).unwrap(),
	)
	.unwrap();
	assert_eq!(scene, source_scene);
	assert_eq!(project.input_profile, source_project.input_profile);
	let mut source = SimulationSession::new(source_project, Arc::new(source_scene)).unwrap();
	let mut packaged = SimulationSession::new(project, Arc::new(scene)).unwrap();
	for session in [&mut source, &mut packaged] {
		session.tick(1. / 60.).unwrap();
		let body = session.character_motion()[0].entity.key.clone();
		session.set_character_input(&body, [0.; 2], true).unwrap();
		session.set_character_input(&body, [0.; 2], false).unwrap();
		for _ in 0..120 {
			session.tick(1. / 60.).unwrap();
		}
		let motion = &session.character_motion()[0];
		assert_eq!(motion.jumped_at_tick, Some(2));
		assert!(motion.landed_at_tick.is_some());
		assert!(motion.grounded);
	}
	assert_eq!(source.world().data(), packaged.world().data());
}
