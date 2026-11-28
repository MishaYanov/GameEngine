//! Observe authored scene routes and streamed geometry through real Vulkan.

use super::*;
use runtime::{
	EntityKey, SceneData,
	components::{ComponentId, NO_MESH_PATH},
	game_camera::{CameraComponent, CameraSettings},
	project::{BehaviorSpace, BehaviorSpec, RegionBounds, SceneRoute, WorldLod, WorldRegion},
	scene_catalog::SceneDescriptor,
	scene_data::{ColliderData, ColliderInteraction, EntityData, TransformData},
	sequence::{SequenceAction, SequenceAsset, SequenceCue, SequenceInstance, SequenceTrigger},
};
use serde_json::json;
use std::{collections::BTreeMap, fs};
use winit::platform::windows::EventLoopBuilderExtWindows;

fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}

fn scene_id(value: &str) -> SceneId {
	SceneId::new(value).unwrap()
}

fn scene(entities: Vec<EntityData>) -> SceneData {
	let mut scene =
		SceneData::from_json(include_str!("../../assets/scenes/camera-room.json")).unwrap();
	scene.entities = entities;
	scene.bindings.clear();
	scene.lights.clear();
	scene.organization = Default::default();
	scene.environment = Default::default();
	scene.camera = Default::default();
	scene.default_game_camera = None;
	scene
}

fn placed(name: &str, position: [f32; 3], mesh: &str) -> EntityData {
	EntityData::new(key(name), name, mesh).with_transform(TransformData {
		position,
		..Default::default()
	})
}

fn marker(name: &str, position: [f32; 3]) -> EntityData {
	placed(name, position, NO_MESH_PATH).with_renderable(false)
}

fn actor(name: &str, x: f32) -> EntityData {
	placed(name, [x, 0.5, 0.], "props/cube.glb").with_collider(Some(ColliderData::Sphere {
		radius: 0.25,
		offset: [0.; 3],
		interaction: Default::default(),
	}))
}

fn sensor() -> EntityData {
	marker("start-sequence", [0., 0.5, 0.]).with_collider(Some(ColliderData::Box {
		half_extents: [0.7; 3],
		offset: [0.; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	}))
}

fn shot(document: &SceneData) -> EntityData {
	let mut camera = marker("shot", [1., 3., 4.]);
	camera.camera = Some(Box::new(CameraComponent {
		id: ComponentId::generate(),
		enabled: true,
		settings: CameraSettings::from_legacy(&document.camera),
	}));
	camera
}

fn sequence(project: &mut ProjectData, scene: &str, name: &str, triggered: bool) {
	project.sequences.assets.push(SequenceAsset {
		id: key(name),
		schema_version: 1,
		duration_ms: 300,
		cues: vec![
			SequenceCue {
				at_ms: 0,
				action: SequenceAction::CameraCut {
					role: key("camera"),
				},
			},
			SequenceCue {
				at_ms: 20,
				action: SequenceAction::Subtitle {
					text: "Arrival".into(),
					duration_ms: 250,
				},
			},
		],
	});
	project.sequences.instances.push(SequenceInstance {
		id: key(name),
		scene_id: scene_id(scene),
		sequence_id: key(name),
		bindings: BTreeMap::from([(key("camera"), key("shot"))]),
		trigger: triggered.then(|| SequenceTrigger {
			sensor: key("start-sequence"),
			actor: key("hero"),
		}),
		once_per_entry: true,
	});
}

fn register(project: &mut ProjectData, root: &Path, name: &str, document: &SceneData) {
	let path = format!("scenes/{name}.json");
	project
		.scenes
		.insert(SceneDescriptor::new(scene_id(name), &path).unwrap())
		.unwrap();
	scene_io::save_data(&root.join(path), document).unwrap();
}

struct Fixture {
	root: PathBuf,
	keep: bool,
}

impl Fixture {
	fn new(label: &str) -> Self {
		let evidence = std::env::var_os("ENGINE_SCENE_FLOW_EVIDENCE").map(PathBuf::from);
		let keep = evidence.is_some();
		let root = evidence.unwrap_or_else(std::env::temp_dir).join(format!(
			"native-{label}-{}",
			runtime::asset_catalog::AssetId::generate().as_str()
		));
		fs::create_dir_all(root.join("assets/scenes")).unwrap();
		fs::create_dir(root.join("assets/props")).unwrap();
		for name in ["cube.glb", "sphere.glb"] {
			fs::copy(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("assets/props")
					.join(name),
				root.join("assets/props").join(name),
			)
				.unwrap();
		}
		Self { root, keep }
	}

	fn assets(&self) -> PathBuf {
		self.root.join("assets")
	}

	fn start(&self, project: ProjectData, events: &ActiveEventLoop) -> GameApp {
		let manifest = self.root.join("project.json");
		scene_io::save_project_data(&manifest, &project).unwrap();
		let (project, assets) =
			if let Some(compiler) = std::env::var_os("ENGINE_SCENE_FLOW_COMPILER") {
				let output = self.root.join("staged");
				fs::create_dir(&output).unwrap();
				let staged = std::process::Command::new(compiler)
					.arg("--stage")
					.arg(&manifest)
					.arg(&output)
					.status()
					.unwrap();
				assert!(staged.success(), "compiler should stage the native fixture");
				let destination = output.join(&project.name);
				(
					ProjectData::load(&destination.join("project.json")).unwrap(),
					destination.join("assets"),
				)
			} else {
				(project, self.assets())
			};
		let window = Arc::new(
			events
				.create_window(
					Window::default_attributes()
						.with_visible(false)
						.with_inner_size(winit::dpi::PhysicalSize::new(160, 120)),
				)
				.unwrap(),
		);
		let mut app = GameApp::default();
		app.project = Some(project);
		app.standalone_runtime = true;
		app.installed_project = true;
		app.smoke_test = true;
		app.initialize_window(window, assets).unwrap();
		app
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		if !self.keep {
			let _ = fs::remove_dir_all(&self.root);
		}
	}
}

fn frame(app: &mut GameApp) -> Result<(), String> {
	let result = app.tick_play(1.0 / 60.0);
	if app
		.scene_transition
		.as_ref()
		.is_some_and(|waiting| !waiting.waiting_frame_drawn)
	{
		app.render().unwrap();
	}
	app.input.end_frame();
	result
}

#[track_caller]
fn wait_for(app: &mut GameApp, mut condition: impl FnMut(&GameApp) -> bool) {
	let deadline = Instant::now() + Duration::from_secs(8);
	while !condition(app) {
		assert!(
			Instant::now() < deadline,
			"native route or region did not finish: status={:?} focus={:?} region_message={:?}",
			app.editor.play_status(),
			app.editor.streaming_focus(),
			app.world_streaming_status
		);
		frame(app).unwrap();
		std::thread::yield_now();
	}
}

fn cutscenes_and_arrival(events: &ActiveEventLoop) {
	let fixture = Fixture::new("scene-flow");
	let root = fixture.assets();
	let mut hall = scene(vec![actor("hero", 0.), sensor()]);
	hall.entities.push(shot(&hall));
	let mut memory = scene(vec![marker("memory-start", [0.; 3])]);
	memory.entities.push(shot(&memory));
	let arena = scene(vec![
		actor("hero", -10.),
		marker("arena-start", [4., 0.5, 1.]),
	]);
	let mut project =
		ProjectData::for_scene_with_id("Story route", scene_id("hall"), "scenes/hall.json", &hall)
			.unwrap();
	scene_io::save_data(&root.join("scenes/hall.json"), &hall).unwrap();
	register(&mut project, &root, "memory", &memory);
	register(&mut project, &root, "arena", &arena);
	sequence(&mut project, "hall", "gate-reveal", true);
	sequence(&mut project, "memory", "flashback", false);
	project.scene_routes = serde_json::from_value::<Vec<SceneRoute>>(json!([
		{"id":"reveal-memory","from":"hall","to":"memory","activate":{"kind":"sequence_completed","instance":"gate-reveal"},
		 "preload":{"kind":"when_sequence_starts","instance":"gate-reveal"},"arrival":"memory-start","start_sequence":"flashback"},
		{"id":"memory-arena","from":"memory","to":"arena","activate":{"kind":"sequence_completed","instance":"flashback"},
		 "preload":{"kind":"when_sequence_starts","instance":"flashback"},"arrival":"arena-start","arrival_actor":"hero"}
	])).unwrap();
	let mut app = fixture.start(project, events);
	wait_for(&mut app, |app| {
		app.editor
			.play_status()
			.unwrap()
			.sequence
			.is_some_and(|view| view.subtitle.is_some())
	});
	assert_eq!(app.editor.play_scene_id(), Some(&scene_id("hall")));
	let run = app.editor.play_status().unwrap().run_id;
	wait_for(&mut app, |app| {
		app.editor.play_scene_id() == Some(&scene_id("memory"))
	});
	wait_for(&mut app, |app| {
		app.editor.play_status().unwrap().sequence.is_some()
	});
	app.input.press(Key::Enter);
	frame(&mut app).unwrap();
	app.input.release(Key::Enter);
	wait_for(&mut app, |app| {
		app.editor.play_scene_id() == Some(&scene_id("arena"))
	});
	assert_eq!(app.editor.play_status().unwrap().run_id, run);
	assert_eq!(
		app.editor.play_world().unwrap().data().entities[0]
			.transform
			.position,
		[4., 0.5, 1.]
	);
	assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
}

fn seamless_regions(events: &ActiveEventLoop) {
	let fixture = Fixture::new("world-regions");
	let root = fixture.assets();
	let world = scene(vec![
		actor("hero", -1.),
		placed("far-c", [20., -0.55, 0.], "props/cube.glb"),
	]);
	let world_id = scene_id("world");
	let mut project = ProjectData::for_scene_with_id(
		"Additive test",
		world_id.clone(),
		"scenes/world.json",
		&world,
	)
		.unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("hero"),
		speed: 8.,
		space: BehaviorSpace::World,
	});
	scene_io::save_data(&root.join("scenes/world.json"), &world).unwrap();
	for (name, x) in [("a", 0.), ("b", 8.), ("c", 16.)] {
		let model = if name == "c" {
			"props/sphere.glb"
		} else {
			"props/cube.glb"
		};
		let mut floor = placed(&format!("floor-{name}"), [x + 4., -0.55, 0.], model);
		floor.transform.scale = [8., 1., 8.];
		floor.collider = Some(ColliderData::Box {
			half_extents: [0.5; 3],
			offset: [0.; 3],
			interaction: Default::default(),
		});
		register(&mut project, &root, name, &scene(vec![floor]));
		project.world_regions.push(WorldRegion {
			world: world_id.clone(),
			scene: scene_id(name),
			bounds: RegionBounds {
				min: [x, -2., -4.],
				max: [x + 8., 3., 4.],
			},
			preload_distance: 4.,
			release_distance: 6.,
		});
	}
	project.world_lods.push(WorldLod {
		world: world_id.clone(),
		detail: key("floor-c"),
		proxy: key("far-c"),
		anchor: [20., 0., 0.],
		detail_distance: 6.,
	});
	let mut app = fixture.start(project, events);
	let initial = app.editor.play_status().unwrap();
	let position = |app: &GameApp| app.editor.streaming_focus().unwrap()[0];
	app.input.press(Key::D);
	wait_for(&mut app, |app| position(app) > 11.);
	app.input.release(Key::D);
	assert!(app.editor.resident_regions().contains(&scene_id("b")));
	assert!(app.scene.entity_by_key(&key("floor-b")).is_some());
	app.input.press(Key::A);
	wait_for(&mut app, |app| position(app) < 6.);
	app.input.release(Key::A);
	app.input.press(Key::D);
	wait_for(&mut app, |app| position(app) > 21.);
	app.input.release(Key::D);
	wait_for(&mut app, |app| {
		!app.editor.resident_regions().contains(&scene_id("a"))
	});
	let current = app.editor.play_status().unwrap();
	assert_eq!(
		(current.run_id, current.scene_generation),
		(initial.run_id, initial.scene_generation)
	);
	assert_eq!(current.scene_id, world_id);
	assert!(app.scene.entity_by_key(&key("floor-a")).is_none());
	assert!(app.scene.entity_by_key(&key("floor-c")).is_some());
	let lods = &app.project.as_ref().unwrap().world_lods;
	let near = app
		.scene
		.try_render_world_detail(None, lods, [20., 1., 0.])
		.unwrap()
		.1;
	let far = app
		.scene
		.try_render_world_detail(None, lods, [0., 1., 0.])
		.unwrap()
		.1;
	assert_eq!(near.draw_calls, far.draw_calls);
	assert!(near.triangles > far.triangles);
	assert_eq!(app.asset_manager.as_ref().unwrap().loaded_model_count(), 2);
	assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);

	// Retiring the last sphere owner must release its submitted draw before pruning.
	app.input.press(Key::A);
	wait_for(&mut app, |app| position(app) < 9.);
	app.input.release(Key::A);
	assert!(!app.editor.resident_regions().contains(&scene_id("c")));
	assert_eq!(app.asset_manager.as_ref().unwrap().loaded_model_count(), 1);
}

#[test]
#[ignore = "requires Windows Vulkan"]
fn cinematic_routes_and_seamless_regions_reach_a_presented_frame() {
	#[derive(Default)]
	struct Check {
		done: bool,
	}
	impl ApplicationHandler for Check {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			cutscenes_and_arrival(events);
			seamless_regions(events);
			self.done = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = Check::default();
	events.run_app(&mut check).unwrap();
	assert!(check.done);
}

fn native_grid_stress(events: &ActiveEventLoop) {
	const EDGE: usize = 7;
	const CELL: f32 = 4.0;
	const MAX_MEASURED_UPDATES: usize = 20_000;

	let fixture = Fixture::new("world-grid-stress");
	let root = fixture.assets();
	let hero = placed("grid-hero", [2.0, 2.0, 0.0], "props/cube.glb");
	let world = scene(vec![hero]);
	let world_id = scene_id("grid-world");
	let mut project = ProjectData::for_scene_with_id(
		"Grid stress",
		world_id.clone(),
		"scenes/grid-world.json",
		&world,
	)
		.unwrap();
	project.add_behavior(BehaviorSpec::ControlledMove {
		entity: key("grid-hero"),
		speed: 12.0,
		space: BehaviorSpace::World,
	});
	scene_io::save_data(&root.join("scenes/grid-world.json"), &world).unwrap();

	let mut route = Vec::with_capacity(EDGE * EDGE);
	for row in 0..EDGE {
		for offset in 0..EDGE {
			let column = if row % 2 == 0 {
				offset
			} else {
				EDGE - 1 - offset
			};
			let x = column as f32 * CELL;
			let y = row as f32 * CELL;
			let name = format!("grid-{row}-{column}");
			let center = [x + CELL / 2.0, y + CELL / 2.0];
			let prop = placed(
				&format!("prop-{name}"),
				[center[0], center[1], -2.0],
				if (row + column) % 2 == 0 {
					"props/cube.glb"
				} else {
					"props/sphere.glb"
				},
			);
			register(&mut project, &root, &name, &scene(vec![prop]));
			let region = scene_id(&name);
			project.world_regions.push(WorldRegion {
				world: world_id.clone(),
				scene: region.clone(),
				bounds: RegionBounds {
					min: [x, y, -4.0],
					max: [x + CELL, y + CELL, 4.0],
				},
				preload_distance: 2.2,
				release_distance: 5.6,
			});
			route.push((region, center));
		}
	}
	let mut app = fixture.start(project, events);
	let original = app.editor.play_status().unwrap();
	let mut previous = route[0].1;
	let mut update_us = Vec::new();
	let mut slowest_update = None;
	let mut longest_update_us = 0;
	let drain_checkpoints = std::env::var_os("ENGINE_SCENE_FLOW_DRAIN_CHECKPOINTS").is_some();
	let mut checkpoint_draw_us = Vec::new();
	let mut checkpoint_wait_us = Vec::new();
	let mut peak_resident = 0;
	let mut peak_gpu_bytes = 0;
	let mut peak_gpu_resources = 0;
	let mut rendered = 0;
	for (visit, (region, target)) in route.iter().chain(route.iter().rev()).enumerate() {
		if *target == previous {
			wait_for(&mut app, |app| {
				app.editor.resident_regions().contains(region)
			});
		} else {
			let axis = usize::from(target[1] != previous[1]);
			let increasing = target[axis] > previous[axis];
			let movement = match (axis, increasing) {
				(0, true) => Key::D,
				(0, false) => Key::A,
				(_, true) => Key::W,
				(_, false) => Key::S,
			};
			app.input.press(movement);
			let deadline = Instant::now() + Duration::from_secs(8);
			loop {
				let focus = app.editor.streaming_focus().unwrap();
				if (increasing && focus[axis] >= target[axis] - 0.02)
					|| (!increasing && focus[axis] <= target[axis] + 0.02)
				{
					break;
				}
				assert!(
					Instant::now() < deadline,
					"failed to reach {} from {focus:?}: {:?}",
					region.as_str(),
					app.world_streaming_status
				);
				assert!(
					update_us.len() < MAX_MEASURED_UPDATES,
					"native update sample limit"
				);
				let before = app.editor.resident_regions();
				let start = Instant::now();
				frame(&mut app).unwrap();
				let elapsed_us = start.elapsed().as_micros();
				let after = app.editor.resident_regions();
				if slowest_update.is_none() || elapsed_us > longest_update_us {
					longest_update_us = elapsed_us;
					slowest_update = Some(json!({
						"toward_region": region,
						"visit_index": visit,
						"loaded": after.iter().filter(|id| !before.contains(id)).collect::<Vec<_>>(),
						"unloaded": before.iter().filter(|id| !after.contains(id)).collect::<Vec<_>>(),
					}));
				}
				update_us.push(elapsed_us);
				peak_resident = peak_resident.max(after.len());
				let assets = app.asset_manager.as_ref().unwrap();
				peak_gpu_bytes = peak_gpu_bytes.max(assets.gpu_logical_bytes());
				peak_gpu_resources = peak_gpu_resources.max(assets.gpu_resource_count());
				std::thread::yield_now();
			}
			app.input.release(movement);
		}
		assert!(app.editor.resident_regions().contains(region));
		assert!(
			app.scene
				.entity_by_key(&key(&format!("prop-{}", region.as_str())))
				.is_some()
		);
		if visit % EDGE == 0 {
			let started = Instant::now();
			assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			checkpoint_draw_us.push(started.elapsed().as_micros());
			if drain_checkpoints {
				let started = Instant::now();
				app.renderer
					.as_ref()
					.unwrap()
					.wait_for_scene_draw()
					.unwrap();
				checkpoint_wait_us.push(started.elapsed().as_micros());
			}
			rendered += 1;
		}
		previous = *target;
	}
	let current = app.editor.play_status().unwrap();
	assert_eq!(
		(current.run_id, current.scene_generation),
		(original.run_id, original.scene_generation)
	);
	assert_eq!(current.scene_id, world_id);
	assert!(peak_resident <= 8, "resident region cap exceeded");
	assert!(peak_gpu_bytes > 0);
	// The two single-mesh GLBs share the default white texture.
	assert_eq!(
		peak_gpu_resources, 3,
		"region attachments should reuse their GPU assets"
	);
	assert!(app.asset_manager.as_ref().unwrap().loaded_model_count() <= 2);
	assert!(
		!app.editor
			.resident_regions()
			.contains(&route.last().unwrap().0)
	);
	assert!(app.scene.entity_by_key(&key("grid-hero")).is_some());
	app.wait_for_gpu().unwrap();
	update_us.sort_unstable();
	let p95 = update_us[(update_us.len() * 95).div_ceil(100) - 1];
	let report = json!({
		"regions": route.len(),
		"visits": route.len() * 2,
		"peak_resident": peak_resident,
		"peak_asset_gpu_logical_bytes": peak_gpu_bytes,
		"peak_asset_gpu_resources": peak_gpu_resources,
		"native_update_samples": update_us.len(),
		"native_update_wall_p95_us": p95,
		"native_update_wall_max_us": update_us.last(),
		"slowest_update": slowest_update,
		"presented_checkpoint_frames": rendered,
		"checkpoint_draw_us": checkpoint_draw_us,
		"checkpoint_wait_us": checkpoint_wait_us,
	});
	println!("native world grid {report}");
	fs::write(
		fixture.root.join("native-world-grid.json"),
		report.to_string(),
	)
		.unwrap();
}

#[test]
#[ignore = "requires Windows Vulkan"]
fn larger_native_world_keeps_region_resources_bounded() {
	#[derive(Default)]
	struct Check {
		done: bool,
	}
	impl ApplicationHandler for Check {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			native_grid_stress(events);
			self.done = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = Check::default();
	events.run_app(&mut check).unwrap();
	assert!(check.done);
}
