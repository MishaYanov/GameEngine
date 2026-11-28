mod support;

use engine_core::{
	Key, SceneData,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	command::{EditorCommandDto, EditorCommandRequest},
	components::{
		ComponentId, InputActionsRef, NO_MESH_PATH, NativeBehaviorComponent, NativeBehaviorSpec,
		ParticleBurstSettings, ParticleEmitterComponent, ParticleEmitterSettings,
		PlayerControllerComponent, PlayerControllerSettings, PlayerMovementSpace,
		ProjectileLauncherSettings, ProjectileSettings, ScriptComponent, ScriptDescriptor,
		ScriptSettings,
	},
	editor::{EditorSession, PrefabData},
	input::{GameInputBinding, InputProfile},
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{ColliderData, ColliderInteraction, EntityData},
	simulation_session::{
		ConfiguredBehaviorFactory, GameModule, GameModuleFactory, ObjectScript,
		ObjectScriptCommands, ObjectScriptContext, ObjectScriptRegistration, ProjectileHit,
		SimulationSession,
	},
};
use std::{collections::HashSet, sync::Arc};
use support::{close, entity, key, scene};

const STEP: f64 = 1.0 / 60.0;
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn behavior(id: &str, spec: NativeBehaviorSpec) -> NativeBehaviorComponent {
	NativeBehaviorComponent {
		id: ComponentId::new(id).unwrap(),
		enabled: true,
		behavior: spec,
	}
}

fn wall(id: &str, position: [f32; 3], half_extents: [f32; 3], layer: u32) -> EntityData {
	entity(id, position).with_collider(Some(ColliderData::Box {
		half_extents,
		offset: [0.; 3],
		interaction: ColliderInteraction {
			layers: layer,
			..Default::default()
		},
	}))
}

fn projectile(lifetime_ticks: u32) -> PrefabData {
	let mut body =
		EntityData::new(key("projectile"), "Projectile", NO_MESH_PATH).with_renderable(false);
	body.native_behaviors.push(behavior(
		"motion",
		NativeBehaviorSpec::Projectile {
			settings: ProjectileSettings {
				speed: 24.,
				lifetime_ticks,
				collision_mask: 2,
				impact: Some(ParticleBurstSettings {
					count: 3,
					settings: ParticleEmitterSettings {
						color: [0.1, 0.2, 1.],
						gravity: 0.,
						speed: 0.,
						lifetime_seconds: 0.25,
						..Default::default()
					},
				}),
			},
		},
	));
	body.particle_emitter = Some(ParticleEmitterComponent {
		id: ComponentId::new("trail").unwrap(),
		enabled: true,
		settings: ParticleEmitterSettings {
			rate_per_second: 60.,
			gravity: 0.,
			speed: 0.,
			linger_on_despawn: true,
			..Default::default()
		},
	});
	PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![body],
		gameplay: None,
		reference_slots: Default::default(),
	}
}

fn launcher() -> EntityData {
	let mut player = wall("player", [0., 0.901, 0.], [0.3, 0.9, 0.3], 1);
	player.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	player.player_controller = Some(Box::new(PlayerControllerComponent {
		id: ComponentId::new("input").unwrap(),
		enabled: true,
		settings: PlayerControllerSettings {
			input: InputActionsRef::ProjectDefault,
			space: PlayerMovementSpace::World,
			local: true,
			control_graph: None,
		},
	}));
	player.native_behaviors.push(behavior(
		"launcher",
		NativeBehaviorSpec::ProjectileLauncher {
			settings: ProjectileLauncherSettings {
				prefab_path: "props/projectile.json".into(),
				action: "launch".into(),
				max_alive: 1,
				spawn_offset: [0., 0., -0.4],
			},
		},
	));
	player
}

#[test]
fn authored_input_launches_once_and_hit_keeps_an_independent_burst() {
	let data = scene(vec![
		launcher(),
		wall("floor", [0., -0.5, 0.], [10., 0.5, 10.], 1),
		wall("ignored", [0., 0.9, -1.0], [0.1, 0.1, 0.03], 1),
		wall("target", [0., 0.9, -2.05], [0.1, 0.1, 0.05], 2),
	]);
	let authored = SceneData::from_json(&data.to_json().unwrap()).unwrap();
	let mut project_template = authored.clone();
	project_template.entities[0].player_controller = None;
	let mut project =
		ProjectData::for_scene("Projectile", "scenes/main.json", &project_template).unwrap();
	project.input_profile = Some(InputProfile {
		actions: vec![
			GameInputBinding {
				id: "launch".into(),
				key: Key::F,
				animation_parameter: None,
			},
			GameInputBinding {
				id: "jump".into(),
				key: Key::Space,
				animation_parameter: None,
			},
		],
		..Default::default()
	});
	project.fixed_hz = 60;
	let mut run = EditorSession::for_project(authored.clone(), project).unwrap();
	run.execute_command(EditorCommandRequest {
		expected_revision: run.revision(),
		command: EditorCommandDto::StartPlay,
	})
		.unwrap();
	run.set_game_keyboard_input_from_codes(&HashSet::from(["KeyF".into()]))
		.unwrap();
	run.tick(STEP).unwrap();
	let pending = run.pending_runtime_spawns();
	assert_eq!(pending.len(), 1);
	let launch = &pending[0];
	assert_eq!(launch.prefab_path, "props/projectile.json");
	assert_eq!(launch.projectile_direction, Some([0., 0., -1.]));
	let prefab = projectile(20);
	let reopened = PrefabData::from_json(&prefab.to_json().unwrap()).unwrap();
	reopened.validate_projectile_launch().unwrap();
	let prepared = run
		.prepare_runtime_spawn(&reopened, HASH, launch.position, Some(launch.id))
		.unwrap();
	let spawned = prepared.entities()[0].key.clone();
	run.commit_runtime_spawn(prepared).unwrap();
	run.tick(0.).unwrap();
	for _ in 0..3 {
		run.tick(STEP).unwrap();
	}
	assert!(run.play_entity_handle(&spawned).is_some());
	assert!(run.particles().any(|sample| sample.position[2] < -1.));
	for _ in 0..3 {
		run.tick(STEP).unwrap();
	}
	assert!(run.play_entity_handle(&spawned).is_none());
	assert!(
		run.pending_runtime_spawns().is_empty(),
		"holding Fire must not launch again"
	);
	let burst = run
		.particles()
		.filter(|sample| sample.color == [0.1, 0.2, 1.])
		.collect::<Vec<_>>();
	assert_eq!(burst.len(), 3);
	for sample in burst {
		close(sample.position[2], -2.);
	}
	assert_eq!(run.authored_document().as_ref(), &authored);
}

#[test]
fn projectile_moves_away_from_its_starting_boundary_and_expires_without_a_hit() {
	let data = Arc::new(scene(vec![wall(
		"behind",
		[0., 0., 0.05],
		[0.1, 0.1, 0.05],
		2,
	)]));
	let mut project = ProjectData::for_scene("Expiry", "scenes/main.json", &data).unwrap();
	project.fixed_hz = 60;
	let mut run = SimulationSession::new(project, data).unwrap();
	let prepared = run
		.prepare_runtime_spawn(&projectile(2), HASH, [0.; 3], None)
		.unwrap();
	let key = prepared.entities()[0].key.clone();
	run.commit_runtime_spawn(prepared).unwrap();
	run.tick(STEP).unwrap();
	run.tick(STEP).unwrap();
	assert!(run.runtime_handle(&key).is_some());
	run.tick(STEP).unwrap();
	assert!(run.runtime_handle(&key).is_none());
	assert!(run.particles().all(|sample| sample.color != [0.1, 0.2, 1.]));
}

#[derive(Clone, Default)]
struct ImpactScript {
	started: bool,
	hit: bool,
}

impl ObjectScript for ImpactScript {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}
	fn on_start(
		&mut self,
		_: &ObjectScriptContext<'_>,
		_: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.started = true;
		Ok(())
	}
	fn on_hit(
		&mut self,
		_: &ObjectScriptContext<'_>,
		hit: &ProjectileHit,
		commands: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		if !self.started
			|| self.hit
			|| hit.target.key != key("surface")
			|| hit.normal != [0., 0., 1.]
		{
			return Err("Unexpected impact callback".into());
		}
		self.hit = true;
		commands.emit_particle_burst(
			hit.position,
			ParticleBurstSettings {
				count: 5,
				settings: ParticleEmitterSettings {
					color: [0.2, 1., 0.3],
					..Default::default()
				},
			},
		)
	}
	fn on_destroy(&mut self, ctx: &ObjectScriptContext<'_>) -> Result<(), String> {
		if !self.hit || (ctx.transform(ctx.owner())?.position[2] + 0.3).abs() > 0.0001 {
			return Err("Destroy must observe the accepted contact position after Hit".into());
		}
		Ok(())
	}
}

struct ImpactFactory;

impl GameModuleFactory for ImpactFactory {
	fn object_scripts(&self) -> &[ObjectScriptRegistration] {
		&[ObjectScriptRegistration {
			descriptor: || ScriptDescriptor {
				id: "test.impact".into(),
				label: "Impact".into(),
				config_version: 1,
				fields: vec![],
				references: vec![],
				receive_local_actions: false,
				write_owner_transform: false,
			},
			instantiate: |_| Ok(Box::new(ImpactScript::default())),
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

#[test]
fn script_runs_start_hit_destroy_and_selects_the_impact_burst() {
	let data = Arc::new(scene(vec![wall(
		"surface",
		[0., 0., -0.35],
		[0.1, 0.1, 0.05],
		2,
	)]));
	let mut project = ProjectData::for_scene("Scripted Impact", "scenes/main.json", &data).unwrap();
	project.fixed_hz = 60;
	let mut run = SimulationSession::with_factory(project, data, &ImpactFactory).unwrap();
	let mut prefab = projectile(10);
	prefab.entities[0].scripts.push(ScriptComponent {
		id: ComponentId::new("impact").unwrap(),
		enabled: true,
		settings: ScriptSettings {
			script_id: "test.impact".into(),
			config_version: 1,
			properties: Default::default(),
			references: Default::default(),
		},
	});
	let prepared = run
		.prepare_runtime_spawn(&prefab, HASH, [0.; 3], None)
		.unwrap();
	run.commit_runtime_spawn(prepared).unwrap();
	run.tick(STEP).unwrap();
	assert!(!run.has_runtime_instances());
	let particles: Vec<_> = run.particles().collect();
	assert_eq!(particles.len(), 5);
	assert!(
		particles
			.iter()
			.all(|sample| sample.color == [0.2, 1., 0.3])
	);
}
