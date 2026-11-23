use std::sync::Arc;

use engine_core::{
	EntityKey, SceneData,
	components::{
		ComponentData, ComponentId, ComponentRecord, ScriptProperty, ScriptReference,
		ScriptSettings,
	},
	project::ProjectData,
	scene_data::{
		CURRENT_SCHEMA_VERSION, ColliderData, ColliderInteraction, EntityData, TransformData,
	},
	simulation_session::SimulationSession,
};

#[test]
fn registered_door_uses_authored_sensor_angle_and_speed() {
	let object = |name: &str, x| {
		EntityData::new(EntityKey::new(name).unwrap(), name, "__no_mesh__")
			.with_renderable(false)
			.with_transform(TransformData {
				position: [x, 0., 0.],
				..Default::default()
			})
	};
	let collider = |sensor| {
		Some(ColliderData::Box {
			half_extents: [0.5; 3],
			offset: [0.; 3],
			interaction: ColliderInteraction {
				sensor,
				..Default::default()
			},
		})
	};
	let sensor = object("door-sensor", 0.).with_collider(collider(true));
	let mut door = object("door", 3.);
	door.put_component(
		ComponentRecord {
			id: ComponentId::new("door-control").unwrap(),
			schema_version: 1,
			enabled: true,
			data: ComponentData::Script {
				settings: ScriptSettings {
					script_id: "project.open_door_on_trigger".into(),
					config_version: 1,
					properties: [
						("open_angle_degrees".into(), ScriptProperty::Number(75.)),
						(
							"speed_degrees_per_second".into(),
							ScriptProperty::Number(60.),
						),
					]
					.into(),
					references: [(
						"sensor".into(),
						ScriptReference::Entity {
							entity: sensor.key.clone(),
						},
					)]
					.into(),
				},
			},
		},
		false,
	)
	.unwrap();
	let document = Arc::new(SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		lights: vec![],
		organization: Default::default(),
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		entities: vec![
			door,
			sensor,
			object("actor", 0.).with_collider(collider(false)),
		],
	});
	let project = ProjectData::for_scene("Script door", "scenes/main.json", &document).unwrap();
	let factory = gameplay_catalog::resolve(&project).unwrap();
	let mut session =
		SimulationSession::with_factory(project.clone(), document.clone(), factory.as_ref())
			.unwrap();
	session.tick(0.018).unwrap();
	assert_eq!(session.world().data().entities[0].transform.rotation[1], 0.);
	session.tick(0.018).unwrap();
	assert!(
		(session.world().data().entities[0].transform.rotation[1].to_degrees() - 1.).abs() < 1e-4
	);
	for _ in 0..85 {
		session.tick(0.018).unwrap();
	}
	assert!(
		(session.world().data().entities[0].transform.rotation[1].to_degrees() - 75.).abs() < 1e-4
	);
	assert_eq!(document.entities[0].transform.rotation[1], 0.);
	let restarted = SimulationSession::with_factory(project, document, factory.as_ref()).unwrap();
	assert_eq!(
		restarted.world().data().entities[0].transform.rotation[1],
		0.
	);
}
