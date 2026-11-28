mod support;

use engine_core::{
	asset_catalog::AssetId,
	command::{EditorCommandDto, EditorCommandRequest},
	components::{
		ComponentId, NO_MESH_PATH, NativeBehaviorComponent, NativeBehaviorSpec,
		ParticleEmitterComponent, ParticleEmitterSettings,
	},
	editor::{EditorSession, PrefabData},
	particles::MAX_PARTICLES,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{EntityData, TransformData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession,
	},
	world::RuntimeWorld,
};
use std::sync::Arc;
use support::{key, scene};

const STEP: f64 = 1.0 / 60.0;

fn emitter(name: &str, position: [f32; 3], settings: ParticleEmitterSettings) -> EntityData {
	let mut entity = EntityData::new(key(name), name, NO_MESH_PATH)
		.with_renderable(false)
		.with_transform(TransformData {
			position,
			..Default::default()
		});
	entity.particle_emitter = Some(ParticleEmitterComponent {
		id: ComponentId::new("emitter").unwrap(),
		enabled: true,
		settings,
	});
	entity
}

fn simulator(entities: Vec<EntityData>) -> SimulationSession {
	let document = scene(entities);
	let project = ProjectData::for_scene("Particles", "scenes/main.json", &document).unwrap();
	SimulationSession::new(project, Arc::new(document)).unwrap()
}

#[test]
fn particle_motion_is_identical_under_different_frame_groupings() {
	let parent = EntityData::new(key("parent"), "Parent", NO_MESH_PATH)
		.with_renderable(false)
		.with_transform(TransformData {
			position: [3., 4., 5.],
			rotation: [0., 0., std::f32::consts::FRAC_PI_2],
			..Default::default()
		});
	let local = emitter(
		"child",
		[1., 0., 0.],
		ParticleEmitterSettings {
			rate_per_second: 30.,
			spread_degrees: 0.,
			..Default::default()
		},
	)
	.with_parent(Some(parent.key.clone()));
	let entities = vec![parent, local];
	let mut single = simulator(entities.clone());
	let mut grouped = simulator(entities);
	for _ in 0..18 {
		single.tick(STEP).unwrap();
	}
	for _ in 0..6 {
		grouped.tick(3.0 * STEP).unwrap();
	}
	let samples: Vec<_> = single.particles().collect();
	assert_eq!(samples, grouped.particles().collect::<Vec<_>>());
	assert_eq!(samples.len(), 9);
	assert!((samples.last().unwrap().position[0] - 3.0).abs() < 0.0001);
	assert!((samples.last().unwrap().position[1] - 5.0).abs() < 0.0001);
	assert!(samples[0].position[0] < 3.0);
	assert!(samples[0].size < samples.last().unwrap().size);
}

#[test]
fn particle_cone_uses_parent_and_local_rotation_without_scale() {
	let samples = |parent_scale, local_scale, spread_degrees| {
		let parent = EntityData::new(key("parent"), "Parent", NO_MESH_PATH)
			.with_renderable(false)
			.with_transform(TransformData {
				position: [3., 4., 5.],
				rotation: [0., 0., std::f32::consts::FRAC_PI_2],
				scale: parent_scale,
			});
		let local = emitter(
			"child",
			[0.; 3],
			ParticleEmitterSettings {
				rate_per_second: 60.,
				gravity: 0.,
				spread_degrees,
				..Default::default()
			},
		)
		.with_parent(Some(parent.key.clone()))
		.with_transform(TransformData {
			rotation: [std::f32::consts::FRAC_PI_4, 0., 0.],
			scale: local_scale,
			..Default::default()
		});
		let mut run = simulator(vec![parent, local]);
		for _ in 0..16 {
			run.tick(STEP).unwrap();
		}
		run.particles().collect::<Vec<_>>()
	};

	for spread in [0., 30.] {
		let uniform = samples([1.; 3], [1.; 3], spread);
		assert_eq!(uniform.len(), 16);
		assert_eq!(uniform, samples([2., 1., 4.], [3., 0.5, 1.], spread));
		assert_eq!(uniform, samples([2., 1., 4.], [0., 0., 0.], spread));
		assert!(uniform[0].position[0] < 3.);
		assert!(uniform[0].position[2] > 5.);
	}
}

#[test]
fn per_emitter_and_scene_limits_drop_excess_births() {
	let settings = ParticleEmitterSettings {
		rate_per_second: 128.,
		lifetime_seconds: 10.0,
		..Default::default()
	};
	let mut single = simulator(vec![emitter("single", [0.; 3], settings.clone())]);
	for _ in 0..180 {
		single.tick(STEP).unwrap();
	}
	assert_eq!(single.particles().count(), 192);

	let mut run = simulator(
		(0..20)
			.map(|index| {
				emitter(
					&format!("emitter-{index:02}"),
					[index as f32 * 10000., 0., 0.],
					settings.clone(),
				)
			})
			.collect(),
	);
	for _ in 0..180 {
		run.tick(STEP).unwrap();
	}
	let samples: Vec<_> = run.particles().collect();
	assert_eq!(samples.len(), MAX_PARTICLES);
	let mut counts = [0; 20];
	for sample in samples {
		let index = (sample.position[0] / 10000.).round() as usize;
		counts[index] += 1;
	}
	assert!(counts.into_iter().all(|count| count <= 192));
}

#[test]
fn a_spawned_emitter_begins_at_its_placement_and_retires_with_its_owner() {
	let mut run = spawn_short_lived_emitter(ParticleEmitterSettings::default());
	assert!(run.tick(STEP).unwrap().particles_changed);
	assert_eq!(run.particles().next().unwrap().position, [2., 3., 4.]);
	let expired = run.tick(STEP).unwrap();
	assert!(expired.particles_changed && expired.changes.topology_changed);
	assert_eq!(run.particles().count(), 0);
}

#[test]
fn an_opted_in_sprite_trail_finishes_after_its_owner_despawns() {
	let sprite = AssetId::generate();
	let mut run = spawn_short_lived_emitter(ParticleEmitterSettings {
		lifetime_seconds: 0.1,
		linger_on_despawn: true,
		sprite_asset_id: Some(sprite.clone()),
		..Default::default()
	});
	run.tick(STEP).unwrap();
	let original = run.particles().next().unwrap();
	assert_eq!(original.position, [2., 3., 4.]);
	assert_eq!(original.sprite_asset_id.as_deref(), Some(&sprite));
	assert!(run.tick(STEP).unwrap().changes.topology_changed);
	let tail: Vec<_> = run.particles().collect();
	assert_eq!(tail.len(), 1);
	assert_eq!(tail[0].sprite_asset_id.as_deref(), Some(&sprite));
	assert!(tail[0].size < original.size);
	for _ in 0..6 {
		run.tick(STEP).unwrap();
	}
	assert_eq!(run.particles().count(), 0);
}

fn spawn_short_lived_emitter(settings: ParticleEmitterSettings) -> SimulationSession {
	let mut spawner =
		EntityData::new(key("spawner"), "Spawner", NO_MESH_PATH).with_renderable(false);
	spawner.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("timed-spawner").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::TimedSpawner {
			prefab_path: "prefabs/spark.json".into(),
			interval_ticks: 2,
			max_alive: 1,
			lifetime_ticks: 2,
			spawn_offset: [2., 3., 4.],
			bindings: Default::default(),
		},
	});
	let mut run = simulator(vec![spawner]);
	run.tick(STEP * 2.0).unwrap();
	let request = run.pending_runtime_spawns().into_iter().next().unwrap();
	let prefab = PrefabData {
		schema_version: 3,
		entities: vec![emitter(
			"spark",
			[0.; 3],
			ParticleEmitterSettings {
				rate_per_second: 60.,
				..settings
			},
		)],
		gameplay: None,
		reference_slots: Default::default(),
		composition: Default::default(),
	};
	let prepared = run
		.prepare_runtime_spawn(&prefab, &"a".repeat(64), request.position, Some(request.id))
		.unwrap();
	run.commit_runtime_spawn(prepared).unwrap();
	run
}

#[test]
fn play_restart_clears_particles_and_restarts_the_seed() {
	let entities = vec![emitter(
		"test",
		[0.; 3],
		ParticleEmitterSettings {
			rate_per_second: 60.,
			..Default::default()
		},
	)];
	let document = scene(entities);
	let project = ProjectData::for_scene("Particles", "scenes/main.json", &document).unwrap();
	let mut editor = EditorSession::for_project(document, project).unwrap();
	let authored = editor.authored_document();
	let execute = |editor: &mut EditorSession, command: EditorCommandDto| {
		editor
			.execute_command(EditorCommandRequest {
				expected_revision: editor.revision(),
				command,
			})
			.unwrap();
	};
	execute(&mut editor, EditorCommandDto::StartPlay);
	let revision = editor.scene_revision();
	let outcome = editor.tick(STEP * 3.0).unwrap();
	assert!(outcome.particles_changed);
	assert!(outcome.changes.is_empty());
	assert_eq!(editor.scene_revision(), revision);
	let samples = editor.particles().collect::<Vec<_>>();
	assert_eq!(samples.len(), 3);
	execute(&mut editor, EditorCommandDto::StopPlay);
	assert_eq!(editor.particles().count(), 0);
	assert_eq!(editor.authored_document(), authored);
	execute(&mut editor, EditorCommandDto::StartPlay);
	editor.tick(STEP * 3.0).unwrap();
	assert_eq!(editor.particles().collect::<Vec<_>>(), samples);
}

struct RejectedSecondStep;

impl GameModuleFactory for RejectedSecondStep {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self))
	}
}

impl GameModule for RejectedSecondStep {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		false
	}

	fn plan(
		&self,
		world: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		out: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let object = &world.data().entities[0];
		if object.transform.position[0] > 0.0 {
			return Err("particle rollback test rejects the second step".into());
		}
		out.push(SimulationAction::SetTransform {
			entity: object.key.clone(),
			transform: TransformData {
				position: [1., 0., 0.],
				..Default::default()
			},
		})
	}
}

#[test]
fn rejected_catch_up_does_not_commit_particle_births() {
	let document = scene(vec![emitter(
		"test",
		[0.; 3],
		ParticleEmitterSettings {
			rate_per_second: 60.,
			..Default::default()
		},
	)]);
	let project = ProjectData::for_scene("Particles", "scenes/main.json", &document).unwrap();
	let mut run =
		SimulationSession::with_factory(project, Arc::new(document), &RejectedSecondStep).unwrap();
	assert!(run.tick(STEP * 2.0).is_err());
	assert_eq!(run.particles().count(), 0);
	run.set_paused(false);
	assert!(run.tick(STEP).unwrap().particles_changed);
	let samples = run.particles().collect::<Vec<_>>();
	assert_eq!(samples[0].position, [1., 0., 0.]);
	assert!(run.tick(STEP).is_err());
	assert_eq!(run.particles().collect::<Vec<_>>(), samples);
}
