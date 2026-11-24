use std::{fs, sync::Arc};

use engine_core::{
	SceneData, asset_catalog::AssetId, project::ProjectData, simulation_session::SimulationSession,
};
use serde_json::json;

#[test]
fn packaged_scene_runs_the_same_authored_object_routine() {
	let fixture =
		std::env::temp_dir().join(format!("object-routine-{}", AssetId::generate().as_str()));
	let _cleanup = Fixture(fixture.clone());
	let assets = fixture.join("assets");
	let exports = fixture.join("exports");
	fs::create_dir_all(assets.join("scenes")).unwrap();
	fs::create_dir(&exports).unwrap();
	let document = json!({
		"schema_version": engine_core::scene_data::CURRENT_SCHEMA_VERSION,
		"bindings": {},
		"camera": engine_core::scene_data::CameraData::default(),
		"entities": [{
			"key": "sign",
			"name": "Sign",
			"transform": { "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
			"components": [{
				"id": "rotation-routine",
				"schema_version": 1,
				"enabled": true,
				"data": {
					"kind": "script",
					"settings": {
						"script_id": "engine.rotation_routine",
						"config_version": 1,
						"properties": {"routine": r#"[{"during":"start","degrees":35},{"during":"fixed_update","degrees":60}]"#}
					}
				}
			}]
		}]
	});
	let authored = SceneData::from_json(&document.to_string()).unwrap();
	fs::write(assets.join("scenes/main.json"), authored.to_json().unwrap()).unwrap();
	let project =
		ProjectData::for_scene("Rotation Routine", "scenes/main.json", &authored).unwrap();
	let prepared = compiler::prepare_project(&project, &assets).unwrap();
	let staged = compiler::stage_prepared_project(&prepared, exports.to_str().unwrap()).unwrap();
	let package =
		compiler::complete_staged_project_with_player(staged, &std::env::current_exe().unwrap())
			.unwrap();
	let packaged_project = ProjectData::load(package.path().join("project.json")).unwrap();
	let package_scene = fs::read_to_string(package.path().join("assets/scenes/main.json")).unwrap();
	let package_scene = SceneData::from_json(&package_scene).unwrap();
	let run = |project: ProjectData, scene| {
		let factory = gameplay_catalog::resolve(&project).unwrap();
		let mut session =
			SimulationSession::with_factory(project, Arc::new(scene), factory.as_ref()).unwrap();
		session.tick(0.018).unwrap();
		session.world().data().entities[0].transform.rotation[1]
	};
	let source_yaw = run(project, authored);
	assert!((source_yaw.to_degrees() - 36.0).abs() < 1e-4);
	assert_eq!(source_yaw, run(packaged_project, package_scene));
}

struct Fixture(std::path::PathBuf);

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
