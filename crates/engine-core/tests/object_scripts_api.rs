mod support;

use std::sync::Arc;

use engine_core::{
	components::{
		ComponentData, ComponentId, ComponentRecord, ScriptDescriptor, ScriptField, ScriptProperty,
		ScriptSettings,
	},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::TransformData,
	simulation_session::{
		ActionWriter, ConfiguredBehaviorFactory, GameModule, GameModuleFactory, InputFrame,
		ObjectScript, ObjectScriptCommands, ObjectScriptContext, ObjectScriptRegistration,
		SimulationAction, SimulationSession,
	},
	world::RuntimeWorld,
};

use support::{entity, scene};

#[derive(Clone)]
struct Counter {
	starts: f32,
	count: f32,
	increment: f32,
	reject_second: bool,
}

impl ObjectScript for Counter {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}
	fn on_start(
		&mut self,
		_: &ObjectScriptContext<'_>,
		_: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.starts += 1.;
		Ok(())
	}
	fn on_fixed_update(
		&mut self,
		_: &ObjectScriptContext<'_>,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.count += 1.;
		if self.reject_second && self.count == 2. {
			return Err("fixture rejects its second step".into());
		}
		commands.set_local_yaw(self.starts + self.count * self.increment)
	}
}

fn descriptor() -> ScriptDescriptor {
	ScriptDescriptor {
		id: "test.counter".into(),
		label: "Counter".into(),
		config_version: 1,
		fields: vec![
			ScriptField::Number {
				key: "increment".into(),
				label: "Increment".into(),
				default: 1.,
				min: 0.,
				max: 2.,
			},
			ScriptField::Boolean {
				key: "reject".into(),
				label: "Reject".into(),
				default: false,
			},
		],
		references: vec![],
		receive_local_actions: false,
		write_owner_transform: true,
	}
}

fn create(settings: &ScriptSettings) -> Result<Box<dyn ObjectScript>, String> {
	let Some(ScriptProperty::Number(increment)) = descriptor().property(settings, "increment")
	else {
		return Err("Missing increment".into());
	};
	Ok(Box::new(Counter {
		starts: 0.,
		count: 0.,
		increment,
		reject_second: matches!(
			settings.properties.get("reject"),
			Some(ScriptProperty::Boolean(true))
		),
	}))
}

struct Factory;

#[derive(Clone)]
struct EventProbe;

impl ObjectScript for EventProbe {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}
	fn needs_update(&self) -> bool {
		false
	}
	fn on_trigger(
		&mut self,
		context: &ObjectScriptContext<'_>,
		edge: &engine_core::simulation_session::ObjectScriptTrigger,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		if edge.phase == engine_core::triggers::TriggerPhase::Enter {
			let yaw = context.transform(context.owner())?.rotation[1];
			commands.set_local_yaw(yaw + 1.)?;
		}
		Ok(())
	}
}

fn event_descriptor() -> ScriptDescriptor {
	let mut desc = descriptor();
	desc.id = "test.trigger".into();
	desc.fields.clear();
	desc.references = vec![engine_core::components::ScriptReferenceField {
		key: "sensor".into(),
		label: "Sensor".into(),
		requires: engine_core::components::ScriptReferenceKind::TriggerCollider,
	}];
	desc
}

fn event_script(_: &ScriptSettings) -> Result<Box<dyn ObjectScript>, String> {
	Ok(Box::new(EventProbe))
}

#[test]
fn trigger_hooks_receive_only_the_selected_sensor_on_the_next_step() {
	use engine_core::{
		EntityKey,
		components::ScriptReference,
		scene_data::{ColliderData, ColliderInteraction},
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
	let mut door = entity("door", [5., 0., 0.]);
	door.put_component(
		ComponentRecord {
			id: ComponentId::new("watch-sensor").unwrap(),
			schema_version: 1,
			enabled: true,
			data: ComponentData::Script {
				settings: ScriptSettings {
					script_id: "test.trigger".into(),
					config_version: 1,
					properties: Default::default(),
					references: [(
						"sensor".into(),
						ScriptReference::Entity {
							entity: EntityKey::new("sensor").unwrap(),
						},
					)]
					.into(),
				},
			},
		},
		false,
	)
	.unwrap();
	let data = scene(vec![
		door,
		entity("sensor", [0.; 3]).with_collider(collider(true)),
		entity("actor", [0.; 3]).with_collider(collider(false)),
		entity("other-sensor", [9., 0., 0.]).with_collider(collider(true)),
		entity("other-actor", [9., 0., 0.]).with_collider(collider(false)),
	]);
	let project = ProjectData::for_scene("Scripts", "scenes/main.json", &data).unwrap();
	let mut game = SimulationSession::with_factory(project, Arc::new(data), &Factory).unwrap();
	assert_eq!(game.tick(0.018).unwrap().trigger_events.len(), 2);
	assert_eq!(game.world().data().entities[0].transform.rotation[1], 0.);
	assert_eq!(game.tick(0.035).unwrap().trigger_events.len(), 0);
	assert_eq!(game.world().data().entities[0].transform.rotation[1], 1.);
}
impl GameModuleFactory for Factory {
	fn object_scripts(&self) -> &[ObjectScriptRegistration] {
		&[
			ObjectScriptRegistration {
				descriptor,
				instantiate: create,
			},
			ObjectScriptRegistration {
				descriptor: event_descriptor,
				instantiate: event_script,
			},
		]
	}
	fn create(
		&self,
		project: &ProjectData,
		scene: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		ConfiguredBehaviorFactory.create(project, scene)
	}
}

fn session(reject: bool) -> SimulationSession {
	let mut data = scene(vec![entity("a", [0., 0., 0.]), entity("b", [0., 0., 0.])]);
	for (index, owner) in data.entities.iter_mut().enumerate() {
		owner
			.put_component(
				ComponentRecord {
					id: ComponentId::new("counter").unwrap(),
					schema_version: 1,
					enabled: true,
					data: ComponentData::Script {
						settings: ScriptSettings {
							script_id: "test.counter".into(),
							config_version: 1,
							properties: [
								(
									"increment".into(),
									ScriptProperty::Number(index as f32 + 1.),
								),
								(
									"reject".into(),
									ScriptProperty::Boolean(index == 0 && reject),
								),
							]
							.into(),
							references: Default::default(),
						},
					},
				},
				false,
			)
			.unwrap();
	}
	let project = ProjectData::for_scene("Scripts", "scenes/main.json", &data).unwrap();
	SimulationSession::with_factory(project, Arc::new(data), &Factory).unwrap()
}

#[test]
fn prepared_scripts_start_once_and_have_per_attachment_state() {
	let mut game = session(false);
	assert_eq!(game.world().data().entities[0].transform.rotation[1], 0.);
	assert_eq!(game.tick(0.035).unwrap().steps, 2);
	let entities = &game.world().data().entities;
	assert_eq!(entities[0].transform.rotation[1], 3.);
	assert_eq!(entities[1].transform.rotation[1], 5.);
	assert_eq!(
		session(false).world().data().entities[0].transform.rotation[1],
		0.
	);
}

#[test]
fn rejected_catch_up_rolls_back_world_and_instance_state() {
	let mut game = session(true);
	assert!(
		game.tick(0.035)
			.unwrap_err()
			.contains("fixture rejects its second step")
	);
	assert_eq!(game.world().data().entities[0].transform.rotation[1], 0.);
	game.set_paused(false);
	assert_eq!(game.tick(0.018).unwrap().steps, 1);
	assert_eq!(game.world().data().entities[0].transform.rotation[1], 2.);
}

#[test]
fn local_yaw_action_preserves_other_transform_edits_in_the_same_step() {
	struct YawAfterTransform;
	impl GameModuleFactory for YawAfterTransform {
		fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
			Ok(Box::new(Self))
		}
	}
	impl GameModule for YawAfterTransform {
		fn needs_tick(&self, _: &InputFrame) -> bool {
			true
		}
		fn plan(
			&self,
			world: &RuntimeWorld,
			_: &InputFrame,
			_: f32,
			out: &mut ActionWriter<'_>,
		) -> Result<(), String> {
			let entity = world.data().entities[0].key.clone();
			out.push(SimulationAction::SetTransform {
				entity: entity.clone(),
				transform: TransformData {
					position: [3., 4., 5.],
					rotation: [0.25, 0., 0.5],
					scale: [2.; 3],
				},
			})?;
			out.push(SimulationAction::SetLocalYaw { entity, radians: 1. })
		}
	}

	let data = scene(vec![entity("door", [0.; 3])]);
	let project = ProjectData::for_scene("Yaw", "scenes/main.json", &data).unwrap();
	let mut game = SimulationSession::with_factory(project, Arc::new(data), &YawAfterTransform).unwrap();
	game.tick(0.018).unwrap();
	let transform = &game.world().data().entities[0].transform;
	assert_eq!(transform.position, [3., 4., 5.]);
	assert_eq!(transform.rotation, [0.25, 1., 0.5]);
	assert_eq!(transform.scale, [2.; 3]);
}
