use std::sync::Arc;

use engine_core::{
	EntityKey, SceneData,
	components::{ComponentData, ComponentId, ComponentRecord, ScriptProperty, ScriptSettings},
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, EntityData},
	simulation_session::SimulationSession,
};

fn scene_with_routine(source: &str, version: u32) -> Arc<SceneData> {
	let mut object = EntityData::new(EntityKey::new("sign").unwrap(), "Sign", "__no_mesh__")
		.with_renderable(false);
	object
		.put_component(
			ComponentRecord {
				id: ComponentId::new("routine").unwrap(),
				schema_version: 1,
				enabled: true,
				data: ComponentData::Script {
					settings: ScriptSettings {
						script_id: "engine.rotation_routine".into(),
						config_version: version,
						properties: [("routine".into(), ScriptProperty::Text(source.into()))]
							.into(),
						references: Default::default(),
					},
				},
			},
			false,
		)
		.unwrap();
	Arc::new(SceneData {
		entities: vec![object],
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		lights: Vec::new(),
		organization: Default::default(),
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
	})
}

#[test]
fn authored_routine_runs_start_and_fixed_update_once_in_the_same_step() {
	let scene = scene_with_routine(
		r#"[{"during":"start","degrees":35},{"during":"fixed_update","degrees":60}]"#,
		1,
	);
	let project = ProjectData::for_scene("Sign", "scenes/main.json", &scene).unwrap();
	let factory = gameplay_catalog::resolve(&project).unwrap();
	let mut session =
		SimulationSession::with_factory(project, scene.clone(), factory.as_ref()).unwrap();
	session.tick(0.018).unwrap();
	assert!(
		(session.world().data().entities[0].transform.rotation[1].to_degrees() - 36.).abs() < 1e-4
	);
	session.tick(0.018).unwrap();
	assert!(
		(session.world().data().entities[0].transform.rotation[1].to_degrees() - 37.).abs() < 1e-4
	);
	assert_eq!(scene.entities[0].transform.rotation[1], 0.);
}

#[test]
fn invalid_rules_and_versions_are_rejected_during_preparation() {
	for (program, version, diagnostic) in [
		(r#"[{"during":"render","degrees":45}]"#, 1, "rule 1"),
		(r#"[{"during":"start","degrees":721}]"#, 1, "Degrees"),
		(r#"[{}, {}, {}, {}, {}, {}, {}]"#, 1, "between 1 and 6"),
		(
			r#"[{"during":"start","degrees":45}]"#,
			2,
			"unsupported script configuration version",
		),
	] {
		let scene = scene_with_routine(program, version);
		let project = ProjectData::for_scene("Sign", "scenes/main.json", &scene).unwrap();
		let factory = gameplay_catalog::resolve(&project).unwrap();
		let failure = SimulationSession::with_factory(project, scene, factory.as_ref())
			.err()
			.unwrap();
		assert!(failure.contains(diagnostic), "{failure}");
	}
}
