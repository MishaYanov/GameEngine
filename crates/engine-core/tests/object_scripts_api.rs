mod support;

use std::sync::Arc;

use engine_core::{
	components::{
		ComponentData, ComponentId, ComponentRecord, ScriptDescriptor, ScriptField, ScriptProperty,
		ScriptSettings,
	},
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{
		ConfiguredBehaviorFactory, GameModule, GameModuleFactory, ObjectScript,
		ObjectScriptCommands, ObjectScriptContext, ObjectScriptRegistration, SimulationSession,
	},
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
impl GameModuleFactory for Factory {
	fn object_scripts(&self) -> &[ObjectScriptRegistration] {
		&[ObjectScriptRegistration {
			descriptor,
			instantiate: create,
		}]
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
