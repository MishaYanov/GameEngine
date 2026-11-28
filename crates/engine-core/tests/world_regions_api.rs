mod support;

use engine_core::{
	EntityKey, Key, SceneData,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::ComponentId,
	game_session::GameSession,
	input::{GameInputBinding, InputProfile},
	project::{BehaviorSpace, BehaviorSpec, ProjectData, RegionBounds, WorldRegion},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::{ColliderData, ColliderInteraction, EntityData},
	simulation_session::{
		ActionWriter, GameModule, GameModuleFactory, InputFrame, SimulationAction,
		SimulationSession, WorldRegionState,
	},
	world::RuntimeWorld,
};
use std::{collections::HashSet, sync::Arc};
use support::{entity, key, scene};

struct Fixture {
	project: ProjectData,
	world: Arc<SceneData>,
	regions: Vec<Arc<SceneData>>,
	ids: Vec<SceneId>,
}

impl Fixture {
	fn new() -> Self {
		let player = entity("player", [-0.5, 0.0, 0.0]).with_collider(Some(ColliderData::Sphere {
			radius: 0.25,
			offset: [0.0; 3],
			interaction: ColliderInteraction::default(),
		}));
		let world = Arc::new(scene(vec![player]));
		let mut project = ProjectData::for_scene("World", "scenes/world.json", &world).unwrap();
		project.fixed_hz = 10;
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: key("player"),
			space: BehaviorSpace::World,
			speed: 10.0,
		});
		let mut ids = Vec::new();
		let mut regions = Vec::new();
		for (index, bounds) in [(-10.0, 0.0), (0.0, 10.0), (10.0, 20.0)]
			.into_iter()
			.enumerate()
		{
			let id = SceneId::new(format!("region-{index}")).unwrap();
			project
				.scenes
				.insert(
					SceneDescriptor::new(id.clone(), format!("scenes/region-{index}.json"))
						.unwrap(),
				)
				.unwrap();
			project.world_regions.push(WorldRegion {
				world: project.startup_scene_id.clone(),
				scene: id.clone(),
				bounds: RegionBounds {
					min: [bounds.0, -10.0, -10.0],
					max: [bounds.1, 10.0, 10.0],
				},
				preload_distance: 1.0,
				release_distance: 2.0,
			});
			let position = if index == 0 {
				[-0.5, 0.0, 0.0]
			} else {
				[bounds.0 + 3.0, 0.0, 0.0]
			};
			let object =
				entity(&format!("gate-{index}"), position).with_collider(Some(ColliderData::Box {
					half_extents: [0.2, 0.5, 0.5],
					offset: [0.0; 3],
					interaction: ColliderInteraction {
						sensor: true,
						..Default::default()
					},
				}));
			ids.push(id);
			regions.push(Arc::new(scene(vec![object])));
		}
		Self {
			project,
			world,
			regions,
			ids,
		}
	}

	fn interlude(&mut self) -> (SceneId, Arc<SceneData>) {
		let id = SceneId::new("interlude").unwrap();
		self.project
			.scenes
			.insert(SceneDescriptor::new(id.clone(), "scenes/interlude.json").unwrap())
			.unwrap();
		(id, Arc::new(scene(Vec::new())))
	}

	fn game(&self) -> GameSession {
		GameSession::with_factory_for_scene(
			Arc::new(self.project.clone()),
			self.project.startup_scene_id.clone(),
			Arc::clone(&self.world),
			Arc::new(DoorModule),
		)
			.unwrap()
	}

	fn play(&self) -> SimulationSession {
		SimulationSession::new(self.project.clone(), Arc::clone(&self.world)).unwrap()
	}

	fn attach(&self, simulation: &mut SimulationSession, index: usize) {
		let candidate = simulation
			.prepare_region_attach(&self.ids[index], Arc::clone(&self.regions[index]))
			.unwrap();
		assert_eq!(candidate.added_entities().len(), 1);
		simulation.commit_region_change(candidate).unwrap();
	}
}

#[test]
fn three_regions_attach_and_retire_without_resetting_the_player_or_triggers() {
	let fixture = Fixture::new();
	let mut simulation = fixture.play();
	let player_key = key("player");
	let player = simulation.runtime_handle(&player_key).unwrap();
	let camera = simulation.camera().clone();

	fixture.attach(&mut simulation, 0);
	let gate = simulation.runtime_handle(&key("gate-0")).unwrap();
	assert_eq!(simulation.resident_regions(), fixture.ids[..1]);
	assert!(simulation.prepare_region_detach(&fixture.ids[0]).is_err());
	assert_eq!(simulation.tick(0.1).unwrap().trigger_events.len(), 1);
	fixture.attach(&mut simulation, 1);
	assert!(simulation.tick(0.1).unwrap().trigger_events.is_empty());

	simulation.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	for _ in 0..5 {
		simulation.tick(0.1).unwrap();
	}
	simulation.clear_input();
	assert_eq!(simulation.streaming_focus().unwrap()[0], 4.5);
	fixture.attach(&mut simulation, 2);
	let detach = simulation.prepare_region_detach(&fixture.ids[0]).unwrap();
	assert_eq!(detach.removed_entities(), &[key("gate-0")]);
	let tick = simulation.tick_index();
	simulation.commit_region_change(detach).unwrap();
	assert_eq!(simulation.tick_index(), tick);
	assert_eq!(simulation.resident_regions(), fixture.ids[1..]);
	assert_eq!(simulation.runtime_handle(&player_key).unwrap(), player);
	assert_eq!(simulation.camera().position, camera.position);
	assert!(simulation.runtime_handle(&gate.key).is_none());
	fixture.attach(&mut simulation, 0);
	let replacement = simulation.runtime_handle(&gate.key).unwrap();
	assert_eq!(replacement.world_id, gate.world_id);
	assert_ne!(replacement.generation, gate.generation);
}

#[test]
fn region_commit_waits_for_native_visibility_and_rejects_stale_tick_candidates() {
	let fixture = Fixture::new();
	let mut simulation = fixture.play();
	simulation
		.set_visibility_capabilities(HashSet::from([key("player")]))
		.unwrap();
	let absent_resources = simulation
		.prepare_region_attach(&fixture.ids[0], Arc::clone(&fixture.regions[0]))
		.unwrap();
	assert!(
		simulation
			.commit_region_change(absent_resources)
			.unwrap_err()
			.contains("render resources")
	);
	assert!(simulation.resident_regions().is_empty());

	let mut stale = simulation
		.prepare_region_attach(&fixture.ids[0], Arc::clone(&fixture.regions[0]))
		.unwrap();
	stale
		.set_visibility_capabilities(HashSet::from([key("player"), key("gate-0")]))
		.unwrap();
	simulation.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	simulation.tick(0.1).unwrap();
	assert!(
		simulation
			.commit_region_change(stale)
			.unwrap_err()
			.contains("stale world")
	);
	assert!(simulation.resident_regions().is_empty());

	let mut admitted = simulation
		.prepare_region_attach(&fixture.ids[0], Arc::clone(&fixture.regions[0]))
		.unwrap();
	admitted
		.set_visibility_capabilities(HashSet::from([key("gate-0")]))
		.unwrap();
	simulation.commit_region_change(admitted).unwrap();
	assert_eq!(simulation.resident_regions(), fixture.ids[..1]);
}

struct DoorModule;

impl GameModuleFactory for DoorModule {
	fn create(&self, _: &ProjectData, _: &SceneId) -> Result<Box<dyn GameModule>, String> {
		Ok(Box::new(Self))
	}
}

impl GameModule for DoorModule {
	fn needs_tick(&self, _: &InputFrame) -> bool {
		true
	}

	fn plan(
		&self,
		world: &RuntimeWorld,
		_: &InputFrame,
		_: f32,
		output: &mut ActionWriter<'_>,
	) -> Result<(), String> {
		let door = key("gate-2");
		if world.entity_index(&door).is_some() {
			output.push(SimulationAction::SetVisible {
				entity: door.clone(),
				visible: false,
			})?;
			output.push(SimulationAction::SetCollisionEnabled {
				entity: door,
				enabled: false,
			})?;
		}
		for entity in &world.data().entities {
			if entity.renderable && entity.key.as_str().starts_with("archive-item-") {
				output.push(SimulationAction::SetVisible {
					entity: entity.key.clone(),
					visible: false,
				})?;
			}
		}
		Ok(())
	}
}

#[test]
fn region_state_roundtrips_and_applies_before_the_region_becomes_live() {
	let fixture = Fixture::new();
	let mut simulation = SimulationSession::with_factory(
		fixture.project.clone(),
		Arc::clone(&fixture.world),
		&DoorModule,
	)
		.unwrap();
	fixture.attach(&mut simulation, 2);
	simulation.tick(0.1).unwrap();
	let archived = simulation.capture_region_state().unwrap();
	let archived = WorldRegionState::from_json(&archived.to_json().unwrap()).unwrap();
	assert!(simulation.restore_region_state(archived.clone()).is_err());
	let detach = simulation.prepare_region_detach(&fixture.ids[2]).unwrap();
	simulation.commit_region_change(detach).unwrap();
	fixture.attach(&mut simulation, 2);
	assert_door_open(&simulation, &key("gate-2"));

	let mut restored = fixture.play();
	restored.restore_region_state(archived).unwrap();
	fixture.attach(&mut restored, 2);
	assert_door_open(&restored, &key("gate-2"));
}

#[test]
fn full_scene_swaps_restore_each_world_and_restart_clears_the_archive() {
	let mut fixture = Fixture::new();
	let (interlude, interlude_document) = fixture.interlude();
	let home = fixture.project.startup_scene_id.clone();
	let mut game = fixture.game();
	fixture.attach(game.active_mut(), 2);
	game.tick(0.1).unwrap();
	assert_door_open(game.active(), &key("gate-2"));
	let snapshot = game.active().capture_region_state().unwrap();

	let exit = game
		.prepare_scene(interlude.clone(), Arc::clone(&interlude_document))
		.unwrap();
	game.commit_scene(exit).unwrap();
	let mut wrong_world = game
		.prepare_new_game(interlude.clone(), Arc::clone(&interlude_document))
		.unwrap();
	assert!(wrong_world.restore_region_state(snapshot.clone()).is_err());
	let returning = game
		.prepare_scene(home.clone(), Arc::clone(&fixture.world))
		.unwrap();
	game.commit_scene(returning).unwrap();
	fixture.attach(game.active_mut(), 2);
	assert_door_open(game.active(), &key("gate-2"));
	assert_eq!(game.active().tick_index(), 0);

	let exit = game
		.prepare_scene(interlude.clone(), Arc::clone(&interlude_document))
		.unwrap();
	game.commit_scene(exit).unwrap();
	let restart = game
		.prepare_new_game(interlude, interlude_document)
		.unwrap();
	game.commit_scene(restart).unwrap();
	let returning = game
		.prepare_scene(home.clone(), Arc::clone(&fixture.world))
		.unwrap();
	game.commit_scene(returning).unwrap();
	fixture.attach(game.active_mut(), 2);
	let index = game.active().world().entity_index(&key("gate-2")).unwrap();
	assert!(game.active().world().data().entities[index].renderable);

	let mut restored = game
		.prepare_new_game(home, Arc::clone(&fixture.world))
		.unwrap();
	restored.restore_region_state(snapshot).unwrap();
	game.commit_scene(restored).unwrap();
	fixture.attach(game.active_mut(), 2);
	assert_door_open(game.active(), &key("gate-2"));
}

#[test]
fn oversized_outgoing_region_state_leaves_scene_commit_atomic() {
	let mut fixture = Fixture::new();
	let (interlude, interlude_document) = fixture.interlude();
	let mut game = fixture.game();
	let suffix = "x".repeat(220);
	let objects = (0..3_300)
		.map(|index| {
			EntityData::new(
				key(&format!("archive-item-{index:04}-{suffix}")),
				format!("Prop {index}"),
				"models/triangle.gltf",
			)
		})
		.collect();
	let data = Arc::new(scene(objects));
	let attached = game
		.active()
		.prepare_region_attach(&fixture.ids[2], data)
		.unwrap();
	game.active_mut().commit_region_change(attached).unwrap();
	game.tick(0.1).unwrap();
	let source = game.active().document();
	let revision = game.progress_revision();
	let candidate = game.prepare_scene(interlude, interlude_document).unwrap();
	let error = game.commit_scene(candidate).unwrap_err();

	assert!(error.contains("byte limit"), "{error}");
	assert_eq!(game.active().scene_id(), &fixture.project.startup_scene_id);
	assert_eq!(game.scene_generation(), 0);
	assert_eq!(game.progress_revision(), revision);
	assert_eq!(game.active().resident_regions(), fixture.ids[2..]);
	assert!(Arc::ptr_eq(&game.active().document(), &source));
}

#[test]
fn a_swept_region_barrier_stops_rapid_crossing_but_allows_retreat_and_tangential_motion() {
	let mut fixture = Fixture::new();
	fixture.project.world_regions[1].bounds.max[0] = 0.25;
	fixture.project.behaviors.clear();
	fixture.project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("player"),
		speed: 40.0,
		space: BehaviorSpace::World,
	});
	let mut simulation = fixture.play();
	let start = simulation.streaming_focus().unwrap();
	simulation.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	assert_eq!(simulation.tick(0.1).unwrap().steps, 1);
	assert_eq!(simulation.streaming_focus().unwrap(), start);
	fixture.attach(&mut simulation, 0);
	simulation.set_controlled_input([1.0, 0.0, 1.0]).unwrap();
	assert_eq!(simulation.tick(0.1).unwrap().steps, 1);
	let seam = simulation.streaming_focus().unwrap();
	assert!(seam[0] < -0.25 && seam[0] > start[0], "{seam:?}");
	assert!(seam[2] > 2.0, "{seam:?}");

	simulation.set_controlled_input([-1.0, 0.0, 0.0]).unwrap();
	assert_eq!(simulation.tick(0.1).unwrap().steps, 1);
	let retreated = simulation.streaming_focus().unwrap();
	assert!(retreated[0] < -3.0, "{retreated:?}");
	fixture.attach(&mut simulation, 1);
	simulation.set_controlled_input([1.0, 0.0, 0.0]).unwrap();
	simulation.tick(0.2).unwrap();
	assert!(simulation.streaming_focus().unwrap()[0] > 1.0);
}

#[test]
fn character_motion_and_support_follow_the_accepted_seam_position() {
	let mut fixture = Fixture::new();
	let data = Arc::make_mut(&mut fixture.world);
	data.entities[0].transform.position = [-0.5, 0.901, 0.0];
	data.entities[0].collider = Some(ColliderData::Box {
		half_extents: [0.25, 0.9, 0.25],
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	});
	data.entities[0].character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("motor").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	let floor = entity("floor", [0.0, -0.5, 0.0]).with_collider(Some(ColliderData::Box {
		half_extents: [20.0, 0.5, 10.0],
		offset: [0.0; 3],
		interaction: ColliderInteraction::default(),
	}));
	data.entities.push(floor);
	fixture.project.behaviors.clear();
	fixture.project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	let mut simulation = fixture.play();
	fixture.attach(&mut simulation, 0);
	simulation
		.set_character_input(&key("player"), [1.0, 0.0], false)
		.unwrap();
	for _ in 0..5 {
		assert_eq!(simulation.tick(0.1).unwrap().steps, 1);
	}
	let player = simulation.world().entity_index(&key("player")).unwrap();
	let edge = simulation.world().data().entities[player]
		.transform
		.position;
	assert!(edge[0] < -0.25 && edge[0] > -0.5, "{edge:?}");
	let motion = simulation.character_motion();
	assert!(motion[0].grounded);
	assert_eq!(motion[0].planar_speed, 0.0);
	simulation
		.set_character_input(&key("player"), [-1.0, 0.0], false)
		.unwrap();
	simulation.tick(0.1).unwrap();
	let retreated = simulation.world().data().entities[player]
		.transform
		.position;
	assert!(retreated[0] < edge[0], "{retreated:?}");

	// A spawn inside a region whose geometry is still missing cannot consume
	// an accepted jump event while its displacement is held.
	Arc::make_mut(&mut fixture.world).entities[0]
		.transform
		.position[0] = 0.5;
	let mut waiting = fixture.play();
	waiting
		.set_character_input(&key("player"), [0.0; 2], true)
		.unwrap();
	assert_eq!(waiting.tick(0.1).unwrap().steps, 1);
	assert_eq!(waiting.character_motion()[0].jumped_at_tick, None);
	assert_eq!(
		waiting.world().data().entities[0].transform.position,
		[0.5, 0.901, 0.0]
	);
}

fn assert_door_open(simulation: &SimulationSession, key: &EntityKey) {
	let index = simulation.world().entity_index(key).unwrap();
	let door = &simulation.world().data().entities[index];
	assert!(!door.renderable);
	assert!(!door.collision_enabled);
}
