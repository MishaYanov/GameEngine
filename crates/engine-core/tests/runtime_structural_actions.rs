mod support;

use engine_core::{
	SceneData,
	components::{ComponentId, ParticleEmitterComponent, ParticleEmitterSettings},
	editor::PrefabData,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{EntityData, TransformData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, RuntimeReparentMode,
		SimulationAction, SimulationSession,
	},
	world::RuntimeWorld,
};
use std::{
	collections::{BTreeMap, VecDeque},
	sync::{Arc, Mutex},
};
use support::{entity, key, scene};

const STEP: f64 = 1.0 / 60.0;
const SOURCE_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Default)]
struct PlannedSteps(Arc<Mutex<VecDeque<Vec<SimulationAction>>>>);

impl PlannedSteps {
	fn push(&self, actions: Vec<SimulationAction>) {
		self.0.lock().unwrap().push_back(actions);
	}
}

impl GameModuleFactory for PlannedSteps {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(self.clone()))
	}
}

impl GameModule for PlannedSteps {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		!self.0.lock().unwrap().is_empty()
	}

	fn plan(
		&self,
		_: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let actions = self.0.lock().unwrap().pop_front().unwrap_or_default();
		for action in actions {
			match action {
				SimulationAction::Reparent {
					target,
					parent,
					mode,
				} => output.reparent_runtime(target, parent, mode)?,
				SimulationAction::SetParticleEmitterEnabled {
					target,
					component_id,
					enabled,
				} => output.set_particle_emitter_enabled(target, component_id, enabled)?,
				action => output.push(action)?,
			}
		}
		Ok(())
	}
}

fn setup(entities: Vec<EntityData>) -> (SimulationSession, PlannedSteps, Arc<SceneData>) {
	let authored = Arc::new(scene(entities));
	let project =
		ProjectData::for_scene("Runtime parenting", "scenes/main.json", &authored).unwrap();
	let steps = PlannedSteps::default();
	let run = SimulationSession::with_factory(project, Arc::clone(&authored), &steps).unwrap();
	(run, steps, authored)
}

fn actor<'a>(run: &'a SimulationSession, name: &str) -> &'a EntityData {
	&run.world().data().entities[run.world().entity_index(&key(name)).unwrap()]
}

fn world_x(run: &SimulationSession, name: &str) -> f32 {
	let index = run.world().entity_index(&key(name)).unwrap();
	run.world().world_matrices()[index].w_axis.x
}

fn topology(run: &SimulationSession) -> u64 {
	run.runtime_inspection("play", "run", 0, 128)
		.unwrap()
		.topology_revision
}

fn prefab() -> PrefabData {
	PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![entity("copy", [0.; 3])],
		gameplay: None,
		reference_slots: BTreeMap::new(),
	}
}

#[test]
fn reparent_modes_commit_at_fixed_steps_and_invalidate_prepared_spawn() {
	let (mut run, steps, authored) = setup(vec![
		entity("box", [1., 0., 0.]),
		entity("detail", [2., 0., 0.]).with_parent(Some(key("box"))),
		entity("anchor", [10., 0., 0.]),
	]);
	let target = run.runtime_handle(&key("box")).unwrap();
	let anchor = run.runtime_handle(&key("anchor")).unwrap();
	let pending = run
		.prepare_runtime_spawn(&prefab(), SOURCE_HASH, [0.; 3], None)
		.unwrap();
	steps.push(vec![SimulationAction::Reparent {
		target: target.clone(),
		parent: Some(anchor),
		mode: RuntimeReparentMode::KeepLocal,
	}]);
	assert_eq!(run.tick(0.0).unwrap().steps, 0);
	assert!(actor(&run, "box").parent.is_none());
	let outcome = run.tick(STEP).unwrap();
	assert!(outcome.changes.full_resync && outcome.changes.topology_changed);
	assert_eq!(topology(&run), 1);
	assert_eq!(run.runtime_handle(&key("box")), Some(target.clone()));
	assert_eq!(actor(&run, "box").parent, Some(key("anchor")));
	assert_eq!(actor(&run, "box").transform.position[0], 1.);
	assert_eq!(world_x(&run, "box"), 11.);
	assert_eq!(world_x(&run, "detail"), 13.);
	assert!(run.commit_runtime_spawn(pending).is_err());

	for expected_topology in [2, 2] {
		steps.push(vec![SimulationAction::Reparent {
			target: target.clone(),
			parent: None,
			mode: RuntimeReparentMode::KeepWorld,
		}]);
		run.tick(STEP).unwrap();
		assert_eq!(topology(&run), expected_topology);
		assert!(actor(&run, "box").parent.is_none());
		assert_eq!(actor(&run, "box").transform.position[0], 11.);
		assert_eq!(world_x(&run, "detail"), 13.);
	}
	assert!(authored.entities[0].parent.is_none());
	assert_eq!(authored.entities[0].transform.position[0], 1.);
}

#[test]
fn later_step_failure_restores_parent_and_pre_step_transforms() {
	let (mut run, steps, authored) = setup(vec![
		entity("box", [1., 0., 0.]),
		entity("anchor", [10., 0., 0.]),
		entity("later", [2., 0., 0.]),
	]);
	let target = run.runtime_handle(&key("box")).unwrap();
	let anchor = run.runtime_handle(&key("anchor")).unwrap();
	steps.push(vec![
		SimulationAction::Move {
			entity: target.key.clone(),
			world_delta: [5., 0., 0.],
		},
		SimulationAction::Reparent {
			target: target.clone(),
			parent: Some(anchor.clone()),
			mode: RuntimeReparentMode::KeepLocal,
		},
	]);
	steps.push(vec![
		SimulationAction::Move {
			entity: key("later"),
			world_delta: [3., 0., 0.],
		},
		SimulationAction::Reparent {
			target: anchor,
			parent: Some(target),
			mode: RuntimeReparentMode::KeepLocal,
		},
	]);
	let before_updates = run.world().matrix_updates();
	let error = run.tick(2.0 * STEP).unwrap_err();
	assert!(error.contains("hierarchy cycle"), "{error}");
	assert_eq!(run.world().data(), authored.as_ref());
	assert_eq!(run.world().matrix_updates(), before_updates);
	assert_eq!(topology(&run), 0);
}

#[test]
fn world_preserving_reparent_rejects_unrepresentable_scale_and_rotation() {
	let parent = entity("anchor", [0.; 3]).with_transform(TransformData {
		rotation: [0.0, 0.0, std::f32::consts::FRAC_PI_4],
		scale: [2.0, 1.0, 1.0],
		..Default::default()
	});
	let (mut run, steps, authored) = setup(vec![entity("box", [1., 0., 0.]), parent]);
	steps.push(vec![SimulationAction::Reparent {
		target: run.runtime_handle(&key("box")).unwrap(),
		parent: run.runtime_handle(&key("anchor")),
		mode: RuntimeReparentMode::KeepWorld,
	}]);
	let error = run.tick(STEP).unwrap_err();
	assert!(error.contains("shear"), "{error}");
	assert_eq!(run.world().data(), authored.as_ref());
	assert_eq!(topology(&run), 0);
}

#[test]
fn reparent_rejects_stale_handles_and_cross_lifetime_parents() {
	for stale in [true, false] {
		let (mut run, steps, _) = setup(vec![entity("authored", [1., 0., 0.])]);
		let authored = run.runtime_handle(&key("authored")).unwrap();
		let pending = run
			.prepare_runtime_spawn(&prefab(), SOURCE_HASH, [0.; 3], None)
			.unwrap();
		let key = pending.entities()[0].key.clone();
		run.commit_runtime_spawn(pending).unwrap();
		let mut spawned = run.runtime_handle(&key).unwrap();
		let expected = if stale {
			spawned.generation += 1;
			"stale"
		} else {
			"spawned lifetime"
		};
		steps.push(vec![SimulationAction::Reparent {
			target: authored,
			parent: Some(spawned),
			mode: RuntimeReparentMode::KeepLocal,
		}]);
		let error = run.tick(STEP).unwrap_err();
		assert!(error.contains(expected), "{error}");
		assert!(actor(&run, "authored").parent.is_none());
		assert_eq!(topology(&run), 1);
	}
}

#[test]
fn despawning_a_spawned_child_keeps_its_authored_parent() {
	let (mut run, steps, _) = setup(vec![entity("authored", [1., 0., 0.])]);
	let authored = run.runtime_handle(&key("authored")).unwrap();
	let pending = run
		.prepare_runtime_spawn(&prefab(), SOURCE_HASH, [0.; 3], None)
		.unwrap();
	let key = pending.entities()[0].key.clone();
	run.commit_runtime_spawn(pending).unwrap();
	let target = run.runtime_handle(&key).unwrap();
	steps.push(vec![SimulationAction::Reparent {
		target: target.clone(),
		parent: Some(authored),
		mode: RuntimeReparentMode::KeepLocal,
	}]);
	run.tick(STEP).unwrap();
	assert_eq!(
		run.world().data().entities[1].parent,
		Some(support::key("authored"))
	);
	steps.push(vec![SimulationAction::Despawn { handle: target }]);
	run.tick(STEP).unwrap();
	assert!(run.runtime_handle(&key).is_none());
	assert_eq!(run.world().data().entities.len(), 1);
}

fn particles(name: &str, enabled: bool) -> EntityData {
	let mut object = entity(name, [0.; 3]);
	object.particle_emitter = Some(ParticleEmitterComponent {
		id: ComponentId::new("emitter").unwrap(),
		enabled,
		settings: ParticleEmitterSettings {
			rate_per_second: 60.0,
			..Default::default()
		},
	});
	object
}

#[test]
fn emitter_enable_disable_is_isolated_and_rolls_back_its_live_particles() {
	let (mut run, steps, authored) = setup(vec![particles("effects", false)]);
	let target = run.runtime_handle(&key("effects")).unwrap();
	let enabled = |enabled| SimulationAction::SetParticleEmitterEnabled {
		target: target.clone(),
		component_id: ComponentId::new("emitter").unwrap(),
		enabled,
	};
	steps.push(vec![enabled(true)]);
	assert!(run.tick(STEP).unwrap().particles_changed);
	let samples = run.particles().collect::<Vec<_>>();
	assert_eq!(samples.len(), 1);
	assert!(
		!authored.entities[0]
			.particle_emitter
			.as_ref()
			.unwrap()
			.enabled
	);

	steps.push(vec![enabled(false)]);
	steps.push(vec![SimulationAction::SetParticleEmitterEnabled {
		target: target.clone(),
		component_id: ComponentId::new("unknown").unwrap(),
		enabled: true,
	}]);
	let error = run.tick(2.0 * STEP).unwrap_err();
	assert!(error.contains("emitter component is missing"), "{error}");
	assert_eq!(run.particles().collect::<Vec<_>>(), samples);
	assert!(
		actor(&run, "effects")
			.particle_emitter
			.as_ref()
			.unwrap()
			.enabled
	);
	assert_eq!(topology(&run), 1);

	run.set_paused(false);
	steps.push(vec![enabled(false)]);
	assert!(run.tick(STEP).unwrap().particles_changed);
	assert_eq!(run.particles().count(), 0);
	assert!(
		!actor(&run, "effects")
			.particle_emitter
			.as_ref()
			.unwrap()
			.enabled
	);
	assert_eq!(run.runtime_handle(&key("effects")), Some(target));
	assert_eq!(topology(&run), 2);
}

#[test]
fn enabling_an_emitter_at_capacity_leaves_the_scene_and_bank_unchanged() {
	let mut objects = (0..engine_core::components::MAX_ENABLED_PARTICLE_EMITTERS)
		.map(|i| particles(&format!("emitter-{i}"), true))
		.collect::<Vec<_>>();
	objects.push(particles("extra", false));
	let (mut run, steps, authored) = setup(objects);
	steps.push(vec![SimulationAction::SetParticleEmitterEnabled {
		target: run.runtime_handle(&key("extra")).unwrap(),
		component_id: ComponentId::new("emitter").unwrap(),
		enabled: true,
	}]);
	let error = run.tick(STEP).unwrap_err();
	assert!(error.contains("enabled Particle Emitters"), "{error}");
	assert_eq!(run.particles().count(), 0);
	assert_eq!(run.world().data(), authored.as_ref());
}

#[test]
fn runtime_reparent_cannot_change_projected_activation() {
	for inactive_child in [false, true] {
		let mut child = entity("box", [1., 0., 0.]);
		let mut parent = entity("anchor", [0.; 3]);
		child.active = !inactive_child;
		parent.active = inactive_child;
		let (mut run, steps, _) = setup(vec![child, parent]);
		steps.push(vec![SimulationAction::Reparent {
			target: run.runtime_handle(&key("box")).unwrap(),
			parent: run.runtime_handle(&key("anchor")),
			mode: RuntimeReparentMode::KeepLocal,
		}]);
		let error = run.tick(STEP).unwrap_err();
		assert!(error.contains("requires active objects"), "{error}");
		assert_eq!(actor(&run, "box").active, !inactive_child);
		assert!(actor(&run, "box").parent.is_none());
		assert_eq!(actor(&run, "anchor").active, inactive_child);
		assert_eq!(topology(&run), 0);
	}
}
