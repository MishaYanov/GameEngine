use std::sync::Arc;

use engine_core::{
	EntityKey, SceneData,
	command::{EditorCommandDto, EditorCommandRequest},
	components::{ComponentData, ComponentId, ComponentRecord},
	editor::EditorSession,
	project::ProjectData,
	scene_data::{CURRENT_SCHEMA_VERSION, ColliderData, EntityData},
	simulation_session::SimulationSession,
	world::RuntimeWorld,
};

#[test]
fn activation_is_inherited_in_play_and_saved_as_an_undoable_authored_edit() {
	let root = EntityKey::new("root").unwrap();
	let child = EntityKey::new("child").unwrap();
	let mut emitter = EntityData::new(child.clone(), "Child", "__no_mesh__")
		.with_parent(Some(root.clone()))
		.with_renderable(false)
		.with_collider(Some(ColliderData::Sphere {
			radius: 0.5,
			offset: [0.0; 3],
			interaction: Default::default(),
		}));
	emitter
		.put_component(
			ComponentRecord {
				id: ComponentId::new("emitter").unwrap(),
				schema_version: 1,
				enabled: true,
				data: ComponentData::ParticleEmitter {
					settings: Default::default(),
				},
			},
			false,
		)
		.unwrap();

	let original = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		entities: vec![
			emitter,
			EntityData::new(root, "Root", "__no_mesh__").with_renderable(false),
		],
		bindings: Default::default(),
		lights: Vec::new(),
		organization: Default::default(),
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
	};
	assert!(
		SceneData::from_json(&original.to_json().unwrap())
			.unwrap()
			.entities[0]
			.active
	);
	let project = ProjectData::for_scene("Activation", "scenes/main.json", &original).unwrap();
	let mut editor = EditorSession::for_project(original.clone(), project.clone()).unwrap();
	let execute = |editor: &mut EditorSession, command| {
		let expected_revision = editor.snapshot().unwrap().revision;
		editor
			.execute(EditorCommandRequest {
				expected_revision,
				command,
			})
			.unwrap();
	};
	execute(
		&mut editor,
		EditorCommandDto::SetObjectActive {
			key: "root".into(),
			active: false,
		},
	);
	let inactive =
		Arc::new(SceneData::from_json(&editor.authored_document().to_json().unwrap()).unwrap());
	assert!(inactive.entities[0].active);
	assert!(!inactive.entities[1].active);
	assert!(inactive.entities[0].collision_enabled);
	assert!(inactive.entities[0].particle_emitter.is_some());

	let play = SimulationSession::new(project, inactive).unwrap();
	let runtime_child = &play.world().data().entities[0];
	assert!(!runtime_child.active);
	assert!(!runtime_child.collision_enabled);
	assert!(runtime_child.particle_emitter.is_none());
	assert_eq!(runtime_child.key, child);
	let mut runtime = RuntimeWorld::new(Arc::new(play.world().data().clone())).unwrap();
	assert!(runtime.set_visible(&child, true).unwrap().is_empty());
	assert!(
		runtime
			.set_collision_enabled(&child, true)
			.unwrap()
			.is_empty()
	);
	assert!(!runtime.data().entities[0].renderable);
	assert!(!runtime.data().entities[0].collision_enabled);

	execute(&mut editor, EditorCommandDto::Undo);
	assert_eq!(editor.authored_document().entities, original.entities);
}
