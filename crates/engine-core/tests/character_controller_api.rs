#[path = "character_controller_api/authoring.rs"]
mod authoring;
#[path = "character_controller_api/input.rs"]
mod input;
#[path = "character_controller_api/motion.rs"]
mod motion;
mod support;
use engine_core::{
	Key,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::{ComponentData, ComponentId, ComponentRecord},
	input::{GameInputBinding, InputProfile},
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::SceneId,
	scene_data::{ColliderData, EntityData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use support::*;
fn box_at(name: &str, position: [f32; 3], half: [f32; 3]) -> EntityData {
	entity(name, position).with_collider(Some(ColliderData::Box {
		half_extents: half,
		offset: [0.; 3],
		interaction: Default::default(),
	}))
}
fn player() -> EntityData {
	let mut e = box_at("player", [0., 0.901, 0.], [0.3, 0.9, 0.3]);
	e.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	e
}
fn room(extra: Vec<EntityData>) -> engine_core::SceneData {
	let mut entities = vec![player(), box_at("floor", [0., -0.5, 0.], [20., 0.5, 20.])];
	entities.extend(extra);
	scene(entities)
}
fn project(data: &engine_core::SceneData) -> ProjectData {
	let mut p = ProjectData::for_scene("Character", "scenes/main.json", data).unwrap();
	p.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	p
}
fn session(data: engine_core::SceneData) -> SimulationSession {
	SimulationSession::new(project(&data), Arc::new(data)).unwrap()
}
fn position(s: &SimulationSession) -> [f32; 3] {
	s.world().data().entities[s.world().entity_index(&key("player")).unwrap()]
		.transform
		.position
}
fn tick(s: &mut SimulationSession) {
	s.tick(1. / 60.).unwrap();
}
#[derive(Clone)]
struct Fail(Arc<AtomicBool>);
impl GameModuleFactory for Fail {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}
impl GameModule for Fail {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if self.0.load(Ordering::Relaxed) {
			out.push(SimulationAction::SetVisible {
				entity: key("missing"),
				visible: false,
			})?;
		}
		Ok(())
	}
}
#[test]
fn late_failure_rolls_back_world_jump_and_motion_snapshot() {
	let data = room(vec![]);
	let flag = Fail(Arc::new(AtomicBool::new(false)));
	let mut s = SimulationSession::with_factory(project(&data), Arc::new(data), &flag).unwrap();
	tick(&mut s);
	let before = s.world().data().clone();
	let motion = serde_json::to_value(s.character_motion()).unwrap();
	s.set_character_input(&key("player"), [1., 0.], true)
		.unwrap();
	flag.0.store(true, Ordering::Relaxed);
	assert!(s.tick(1. / 60.).is_err());
	assert_eq!(&before, s.world().data());
	assert_eq!(motion, serde_json::to_value(s.character_motion()).unwrap());
	assert!(s.paused());
}

#[derive(Clone)]
enum BoundaryAction {
	Collider(engine_core::EntityKey, bool),
	Remove(engine_core::runtime_lifecycle::RuntimeEntityHandle),
}
#[derive(Clone, Default)]
struct BoundaryScript(Arc<std::sync::Mutex<Option<BoundaryAction>>>);
impl GameModuleFactory for BoundaryScript {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}
impl GameModule for BoundaryScript {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		if let Some(action) = self.0.lock().unwrap().clone() {
			out.push(match action {
				BoundaryAction::Collider(entity, enabled) => {
					SimulationAction::SetCollisionEnabled { entity, enabled }
				}
				BoundaryAction::Remove(handle) => SimulationAction::Despawn { handle },
			})?;
		}
		Ok(())
	}
}
