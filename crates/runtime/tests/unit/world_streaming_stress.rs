use super::*;
use crate::{
	EntityKey,
	project::RegionBounds,
	scene_catalog::SceneDescriptor,
	scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData, TransformData},
};
use engine_core::{
	Key,
	character_controller::{CharacterControllerComponent, CharacterControllerSettings},
	components::ComponentId,
	input::{GameInputBinding, InputProfile},
	project::{BehaviorSpace, BehaviorSpec},
	scene_data::ColliderData,
	simulation_session::SimulationSession,
};
use std::{
	collections::HashMap,
	fs, thread,
	time::{Duration, Instant},
};

const GRID_EDGE: usize = 7;
const CELL_WIDTH: f32 = 10.0;

struct Grid {
	root: PathBuf,
	project: ProjectData,
	world: SceneId,
}

impl Grid {
	fn new() -> Self {
		let root =
			std::env::temp_dir().join(format!("scene-world-stress-{}", uuid::Uuid::new_v4()));
		fs::create_dir(&root).unwrap();
		let world = SceneId::new("persistent-world").unwrap();
		let document = cell_document("anchor", [0.0; 3]);
		let mut project =
			ProjectData::for_scene_with_id("World stress", world.clone(), "base.json", &document)
				.unwrap();
		fs::write(root.join("base.json"), document.to_json().unwrap()).unwrap();
		for row in 0..GRID_EDGE {
			for column in 0..GRID_EDGE {
				let id = cell_id(row, column);
				let path = format!("{}.json", id.as_str());
				let content = cell_document(id.as_str(), center(row, column));
				fs::write(root.join(&path), content.to_json().unwrap()).unwrap();
				project
					.scenes
					.insert(SceneDescriptor::new(id.clone(), path).unwrap())
					.unwrap();
				project.world_regions.push(WorldRegion {
					world: world.clone(),
					scene: id,
					bounds: RegionBounds {
						min: [column as f32 * CELL_WIDTH, -2.0, row as f32 * CELL_WIDTH],
						max: [
							(column + 1) as f32 * CELL_WIDTH,
							2.0,
							(row + 1) as f32 * CELL_WIDTH,
						],
					},
					preload_distance: 6.0,
					release_distance: 14.0,
				});
			}
		}
		project.validate().unwrap();
		Self {
			root,
			project,
			world,
		}
	}
}

impl Drop for Grid {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.root);
	}
}

fn cell_document(key: &str, position: [f32; 3]) -> SceneData {
	let camera = crate::Camera::default();
	SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: vec![
			EntityData::new(EntityKey::new(key).unwrap(), key, "props/cube.glb").with_transform(
				TransformData {
					position,
					..Default::default()
				},
			),
		],
		lights: Vec::new(),
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
	}
}

fn cell_id(row: usize, column: usize) -> SceneId {
	SceneId::new(format!("cell-{row}-{column}")).unwrap()
}

fn center(row: usize, column: usize) -> [f32; 3] {
	[
		(column as f32 + 0.5) * CELL_WIDTH,
		0.0,
		(row as f32 + 0.5) * CELL_WIDTH,
	]
}

#[derive(Default)]
struct Visits {
	resident: HashMap<SceneId, Arc<SceneData>>,
	attached: usize,
	detached: usize,
	peak_resident: usize,
}

impl Visits {
	fn reach(
		&mut self,
		grid: &Grid,
		streaming: &mut WorldStreaming,
		focus: [f32; 3],
		target: &SceneId,
	) {
		let started = Instant::now();
		loop {
			let ids = self.resident.keys().cloned().collect::<Vec<_>>();
			match streaming
				.poll(&grid.project, &grid.world, focus, &ids, |_| true)
				.unwrap()
			{
				Some(RegionOperation::Attach { region, document }) => {
					grid.project
						.validate_region_document(&region, &document)
						.unwrap();
					assert_eq!(document.entities[0].key.as_str(), region.as_str());
					assert!(
						self.resident.insert(region, document).is_none(),
						"duplicate attach"
					);
					self.attached += 1;
				}
				Some(RegionOperation::Detach { region }) => {
					let document = self
						.resident
						.remove(&region)
						.expect("detach an attached region");
					let last_owned_document = Arc::downgrade(&document);
					drop(document);
					assert_eq!(last_owned_document.strong_count(), 0);
					self.detached += 1;
				}
				None if self.resident.contains_key(target) && !streaming.waiting() => break,
				None => thread::sleep(Duration::from_millis(1)),
			}
			self.peak_resident = self.peak_resident.max(self.resident.len());
			assert!(self.peak_resident <= MAX_RESIDENT);
			assert!(
				started.elapsed() < Duration::from_secs(5),
				"streaming did not reach {} inside the stress-test time limit",
				target.as_str()
			);
		}
	}
}

#[test]
fn branching_grid_retires_cells_as_the_player_traverses_and_revisits() {
	let grid = Grid::new();
	let mut streaming = WorldStreaming::new(&grid.root).unwrap();
	streaming.enter_world("grid-run", 1);
	let mut visits = Visits::default();
	let mut route = Vec::new();
	for row in 0..GRID_EDGE {
		for offset in 0..GRID_EDGE {
			let column = if row % 2 == 0 {
				offset
			} else {
				GRID_EDGE - 1 - offset
			};
			route.push((row, column));
		}
	}
	for &(row, column) in route.iter().chain(route.iter().rev()) {
		visits.reach(
			&grid,
			&mut streaming,
			center(row, column),
			&cell_id(row, column),
		);
	}
	assert!(
		visits.attached > GRID_EDGE * GRID_EDGE,
		"returning should reload some earlier cells"
	);
	assert_eq!(visits.attached, visits.detached + visits.resident.len());
	assert!(visits.resident.contains_key(&cell_id(0, 0)));
	assert!(
		!visits
			.resident
			.contains_key(&cell_id(GRID_EDGE - 1, GRID_EDGE - 1))
	);
	println!(
		"world streaming stress: {} authored cells, {} visits, {} attaches, {} detaches, {} peak resident",
		grid.project.world_regions.len(),
		route.len() * 2,
		visits.attached,
		visits.detached,
		visits.peak_resident
	);
}

#[test]
fn a_region_pinned_by_a_remote_actor_does_not_stall_streaming() {
	let mut grid = Grid::new();
	let pinned = cell_id(0, 0);
	let old = cell_id(1, 1);
	let target = cell_id(GRID_EDGE - 1, GRID_EDGE - 1);
	let focus = center(GRID_EDGE - 1, GRID_EDGE - 1);
	let mut world = cell_document("player", focus);
	let mut actor = cell_document("remote-actor", center(0, 0))
		.entities
		.pop()
		.unwrap()
		.with_collider(Some(ColliderData::Box {
			half_extents: [0.3, 0.9, 0.3],
			offset: [0.0; 3],
			interaction: Default::default(),
		}));
	actor.character_controller = Some(CharacterControllerComponent {
		id: ComponentId::new("character").unwrap(),
		enabled: true,
		settings: CharacterControllerSettings::default(),
	});
	world.entities.push(actor);
	grid.project.add_behavior(BehaviorSpec::ControlledMove {
		entity: EntityKey::new("player").unwrap(),
		space: BehaviorSpace::World,
		speed: 10.0,
	});
	grid.project.input_profile = Some(InputProfile {
		actions: vec![GameInputBinding {
			id: "jump".into(),
			key: Key::Space,
			animation_parameter: None,
		}],
		..Default::default()
	});
	let mut session = SimulationSession::new(grid.project.clone(), Arc::new(world)).unwrap();
	for (region, position) in [(&pinned, center(0, 0)), (&old, center(1, 1))] {
		let prepared = session
			.prepare_region_attach(region, Arc::new(cell_document(region.as_str(), position)))
			.unwrap();
		session.commit_region_change(prepared).unwrap();
	}
	assert_eq!(session.streaming_focus(), Some(focus));
	assert!(session.prepare_region_detach(&pinned).is_err());

	let mut streaming = WorldStreaming::new(&grid.root).unwrap();
	streaming.enter_world("pinned-region-run", 1);
	let started = Instant::now();
	loop {
		let resident = session.resident_regions();
		match streaming
			.poll(&grid.project, &grid.world, focus, &resident, |region| {
				session.can_detach_region(region)
			})
			.unwrap()
		{
			Some(RegionOperation::Detach { region }) => {
				assert_eq!(region, old);
				let prepared = session.prepare_region_detach(&region).unwrap();
				session.commit_region_change(prepared).unwrap();
			}
			Some(RegionOperation::Attach { region, document }) => {
				assert_eq!(region, target);
				let prepared = session.prepare_region_attach(&region, document).unwrap();
				session.commit_region_change(prepared).unwrap();
				break;
			}
			None => thread::sleep(Duration::from_millis(1)),
		}
		assert!(started.elapsed() < Duration::from_secs(5));
	}
	assert_eq!(session.resident_regions(), vec![pinned, target]);
}

#[test]
fn a_failed_detach_yields_to_other_regions_and_can_retry() {
	let grid = Grid::new();
	let failed = cell_id(0, 0);
	let old = cell_id(1, 1);
	let adjacent = cell_id(GRID_EDGE - 1, GRID_EDGE - 2);
	let target = cell_id(GRID_EDGE - 1, GRID_EDGE - 1);
	let focus = center(GRID_EDGE - 1, GRID_EDGE - 1);
	let mut session = SimulationSession::new(
		grid.project.clone(),
		Arc::new(cell_document("anchor", focus)),
	)
		.unwrap();
	let attach = |session: &mut SimulationSession, id: &SceneId, position| {
		let prepared = session
			.prepare_region_attach(id, Arc::new(cell_document(id.as_str(), position)))
			.unwrap();
		session.commit_region_change(prepared).unwrap();
	};
	attach(&mut session, &failed, center(0, 0));
	attach(&mut session, &old, center(1, 1));

	let mut streaming = WorldStreaming::new(&grid.root).unwrap();
	streaming.enter_world("failed-detach-run", 1);
	let poll = |stream: &mut WorldStreaming, session: &SimulationSession| {
		stream
			.poll(
				&grid.project,
				&grid.world,
				focus,
				&session.resident_regions(),
				|region| session.can_detach_region(region),
			)
			.unwrap()
	};
	assert!(matches!(
		poll(&mut streaming, &session),
		Some(RegionOperation::Detach { region }) if region == failed
	));
	let stale = session.prepare_region_detach(&failed).unwrap();
	attach(
		&mut session,
		&adjacent,
		center(GRID_EDGE - 1, GRID_EDGE - 2),
	);
	assert!(
		session
			.commit_region_change(stale)
			.unwrap_err()
			.contains("stale world")
	);
	streaming.defer_detach(failed.clone());
	assert!(streaming.waiting());
	assert!(matches!(
		poll(&mut streaming, &session),
		Some(RegionOperation::Detach { region }) if region == old
	));
	let prepared = session.prepare_region_detach(&old).unwrap();
	session.commit_region_change(prepared).unwrap();

	let started = Instant::now();
	loop {
		match poll(&mut streaming, &session) {
			Some(RegionOperation::Attach { region, document }) => {
				assert_eq!(region, target);
				let prepared = session.prepare_region_attach(&region, document).unwrap();
				session.commit_region_change(prepared).unwrap();
				break;
			}
			Some(RegionOperation::Detach { region }) => {
				panic!("unexpected detach while the failed region is deferred: {region:?}");
			}
			None => thread::sleep(Duration::from_millis(1)),
		}
		assert!(started.elapsed() < DETACH_RETRY_DELAY);
	}
	assert!(session.resident_regions().contains(&failed));
	*streaming.deferred_detaches.get_mut(&failed).unwrap() = Instant::now();
	assert!(matches!(
		poll(&mut streaming, &session),
		Some(RegionOperation::Detach { region }) if region == failed
	));
}

#[test]
fn calibrated_guard_holds_at_the_seam_and_far_arrival_waits_for_confirmation() {
	let grid = Grid::new();
	let mut streaming = WorldStreaming::new(&grid.root).unwrap();
	streaming.enter_world("arrival-run", 1);
	let starting_cell = cell_id(0, 0);
	let mut visits = Visits::default();
	visits.reach(&grid, &mut streaming, center(0, 0), &starting_cell);
	assert!(!streaming.entry_blocked(
		&grid.project,
		&grid.world,
		[8.75, 0.0, 5.0],
		&[starting_cell.clone()],
	));
	assert!(streaming.entry_blocked(
		&grid.project,
		&grid.world,
		[9.25, 0.0, 5.0],
		&[starting_cell],
	));

	let far_cell = cell_id(GRID_EDGE - 1, GRID_EDGE - 1);
	streaming
		.prepare_arrival(center(GRID_EDGE - 1, GRID_EDGE - 1))
		.unwrap();
	visits.reach(&grid, &mut streaming, center(0, 0), &far_cell);
	let ids = visits.resident.keys().cloned().collect::<Vec<_>>();
	assert!(streaming.arrival_ready(&grid.project, &grid.world, &ids));
	streaming.enter_world("restart", 1);
	assert!(!streaming.arrival_ready(&grid.project, &grid.world, &ids));
}
