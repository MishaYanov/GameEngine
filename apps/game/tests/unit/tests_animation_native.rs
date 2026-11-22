//! Real Vulkan acceptance, hidden window, and the ordinary Player restart path.
use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires native Vulkan and a hidden window server"]
fn animated_characters_draw_independent_palettes_preview_and_restart() {
	let mut config = engine_logger::Config::new("animation-native-test");
	config.console = false;
	config.error_backtraces = false;
	config.capacity = 4096;
	let _logging = engine_logger::init(config).unwrap();
	#[derive(Default)]
	struct Check {
		complete: bool,
	}
	fn draw_all(app: &mut GameApp) {
		let renderer = app.renderer.as_mut().unwrap();
		let [width, height] = renderer.prepare().unwrap().unwrap();
		renderer.set_lights(&app.scene.renderer_lights()).unwrap();
		let camera = renderer::vulkan::CameraUniform::perspective(
			[0., 4., 8.],
			[0., 1., 0.],
			[0., 1., 0.],
			std::f32::consts::FRAC_PI_3,
			width as f32 / height as f32,
			0.1,
			200.,
		);
		let (objects, _) = app.scene.try_render_visible(None).unwrap();
		renderer.draw(&camera, &objects).unwrap();
		let metrics = renderer.last_metrics();
		assert_eq!(
			metrics.skinned_draw_calls,
			app.editor.animations().unwrap().poses().count() as u64 * 2,
			"each instance has two material primitives"
		);
		assert_eq!(
			metrics.skin_palette_matrices,
			app.editor.animations().unwrap().poses().count() as u64 * 2,
			"each instance has one shared two-joint palette"
		);
	}
	impl ApplicationHandler for Check {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let fixture = test_project::ProjectFixture::new("animation-room");
			let manifest = std::env::var_os("GAME_ENGINE_QA_ANIMATION_PROJECT")
				.map(PathBuf::from)
				.unwrap_or_else(|| fixture.manifest_path())
				.canonicalize()
				.unwrap();
			let project = ProjectData::load(&manifest).unwrap();
			let root = manifest.parent().unwrap().join("assets");
			let startup = root.join(project.startup_scene_path().unwrap());
			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(640, 480)),
					)
					.unwrap(),
			);
			let renderer = WindowRenderer::new(window.clone(), 640, 480).unwrap();
			let device = renderer.physical_device_info();
			println!(
				"ANIMATION_NATIVE_DEVICE name={} api={} driver_raw={} vendor={} device={}",
				device.name,
				renderer::vulkan::format_api_version(device.api_version),
				device.driver_version,
				device.vendor_id,
				device.device_id
			);
			check_animated_import(&renderer, &root);
			let mut loaded = scene_io::load(&startup, &root, &renderer).unwrap();
			check_invalid_animated_placement(&mut loaded, &renderer);
			let mut editor =
				SceneEditor::for_project(&loaded.scene, &loaded.camera, project.clone()).unwrap();
			editor.start_play(&mut loaded.scene).unwrap();
			let initial = editor.animations().unwrap().replay_state();
			let mut app = GameApp::default();
			app.scene = loaded.scene;
			app.camera = loaded.camera;
			app.editor = editor;
			app.asset_manager = Some(loaded.assets);
			app.renderer = Some(renderer);
			app.window = Some(window);
			app.asset_root = Some(root);
			app.scene_path = Some(startup);
			app.project_path = Some(manifest);
			app.project_name = Some(project.name.clone());
			app.project = Some(project);
			app.standalone_runtime = true;
			app.installed_project = true;
			let player = runtime::EntityKey::new("player").unwrap();
			let demo = runtime::EntityKey::new("clip-demo").unwrap();
			let baseline = {
				let assets = app.asset_manager.as_ref().unwrap();
				(assets.gpu_logical_bytes(), assets.gpu_resource_count())
			};
			draw_all(&mut app);
			for _ in 0..12 {
				app.tick_play(1. / 60.).unwrap();
				app.input.end_frame();
			}
			assert_eq!(
				app.editor.animations().unwrap().pose(&player).unwrap().mode,
				runtime::animation::MotionMode::Idle
			);
			app.input.press(Key::W);
			for _ in 0..12 {
				app.tick_play(1. / 60.).unwrap();
				app.input.end_frame();
				draw_all(&mut app);
			}
			let bank = app.editor.animations().unwrap();
			assert_eq!(
				bank.pose(&player).unwrap().mode,
				runtime::animation::MotionMode::Walk
			);
			assert_ne!(
				bank.pose(&player).unwrap().elapsed_ticks,
				bank.pose(&demo).unwrap().elapsed_ticks
			);
			app.input.press(Key::Space);
			app.tick_play(1. / 60.).unwrap();
			app.input.end_frame();
			app.input.release(Key::Space);
			assert_eq!(
				app.editor.animations().unwrap().pose(&player).unwrap().mode,
				runtime::animation::MotionMode::Air
			);
			draw_all(&mut app);
			// Preview pins and samples independently; it cannot advance the bank,
			// mutate authored transforms, or leak into the following normal draw.
			let state = app.editor.animations().unwrap().replay_state();
			let before = app.scene.to_data(&app.camera).unwrap();
			let id = app.scene.entity_by_key(&player).unwrap();
			let palette = app
				.scene
				.get(id)
				.unwrap()
				.animation_palette()
				.unwrap()
				.to_vec();
			let asset = app
				.editor
				.animations()
				.unwrap()
				.pose(&player)
				.unwrap()
				.asset
				.clone();
			let preview =
				runtime::animation::sample_pose(asset, 1, 0.5, glam::Mat4::IDENTITY, 99).unwrap();
			app.scene
				.set_animation_pose(&player, Some(&preview))
				.unwrap();
			draw_all(&mut app);
			assert_ne!(
				app.scene.get(id).unwrap().animation_palette().unwrap(),
				palette
			);
			assert_eq!(app.editor.animations().unwrap().replay_state(), state);
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), before);
			app.scene.reset_animation_overrides();
			assert_eq!(
				app.scene.get(id).unwrap().animation_palette().unwrap(),
				palette
			);
			app.input.reset();
			for _ in 0..120 {
				app.tick_play(1. / 60.).unwrap();
				app.input.end_frame();
			}
			assert_eq!(
				app.editor.animations().unwrap().pose(&player).unwrap().mode,
				runtime::animation::MotionMode::Idle
			);
			check_animated_spawn_retirement(&mut app, draw_all, baseline);
			app.renderer.as_ref().unwrap().wait_idle().unwrap();
			app.editor.stop_play(&mut app.scene).unwrap();
			let authored_palette = app
				.scene
				.get(id)
				.unwrap()
				.animation_palette()
				.unwrap()
				.to_vec();
			app.editor.start_play(&mut app.scene).unwrap();
			assert_eq!(
				app.scene.get(id).unwrap().animation_palette().unwrap(),
				authored_palette
			);
			assert_eq!(app.editor.animations().unwrap().replay_state(), initial);
			app.restart_player_game().unwrap();
			assert_eq!(app.editor.animations().unwrap().replay_state(), initial);
			draw_all(&mut app);
			let assets = app.asset_manager.as_ref().unwrap();
			assert_eq!(
				(assets.gpu_logical_bytes(), assets.gpu_resource_count()),
				baseline
			);
			app.renderer.as_ref().unwrap().wait_idle().unwrap();
			drop(app);
			self.complete = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = Check::default();
	events.run_app(&mut check).unwrap();
	assert!(check.complete);
	let log = engine_logger::snapshot(engine_logger::SnapshotRequest {
		limit: 4096,
		..Default::default()
	});
	let errors: Vec<_> = log
		.entries
		.iter()
		.filter(|e| {
			e.component == "renderer::vulkan::validation" && e.level == engine_logger::Level::Error
		})
		.map(|e| e.message.as_str())
		.collect();
	assert!(errors.is_empty(), "Vulkan validation errors: {errors:#?}");
}

fn check_invalid_animated_placement(
	loaded: &mut runtime::scene_io::LoadedScene,
	renderer: &WindowRenderer,
) {
	let before = loaded.scene.to_data(&loaded.camera).unwrap();
	let mut invalid = before.clone();
	let entity = invalid
		.entities
		.iter_mut()
		.find(|e| e.key.as_str() == "clip-demo")
		.unwrap();
	let key = entity.key.clone();
	entity.transform.scale = [1., 2., 1.];
	assert!(scene_io::instantiate(&invalid, &mut loaded.assets, renderer).is_err());
	assert!(
		loaded
			.scene
			.synchronize(&invalid)
			.unwrap_err()
			.contains("positive uniform")
	);
	let change = runtime::world::WorldChange {
		changed_entities: vec![key.clone()],
		..Default::default()
	};
	assert!(
		loaded
			.scene
			.synchronize_authored(invalid.validated().unwrap(), &change)
			.unwrap_err()
			.contains("positive uniform")
	);
	assert_eq!(loaded.scene.to_data(&loaded.camera).unwrap(), before);
	let mut direct = loaded.scene.clone();
	let id = direct.entity_by_key(&key).unwrap();
	direct.get_mut(id).unwrap().transform_mut().scale = [-1., 1., 1.];
	assert!(direct.try_render_visible(None).is_err());
}

fn check_animated_import(renderer: &WindowRenderer, source_root: &std::path::Path) {
	use runtime::assets::project_assets::prepare_import;
	let temporary = std::env::temp_dir().join(format!(
		"animation-native-import-{}",
		runtime::asset_catalog::AssetId::generate().as_str()
	));
	std::fs::create_dir(&temporary).unwrap();
	let source = source_root.join("characters/two-joint.glb");
	let mut assets = AssetManager::new(&temporary).unwrap();
	let prepared = prepare_import(&temporary, &source, "imported.glb").unwrap();
	let gpu = assets.prepare_gpu_asset(renderer, &prepared).unwrap();
	assert_eq!(assets.loaded_model_count(), 1);
	renderer.wait_idle().unwrap();
	assets.discard_gpu_asset(gpu);
	assets.prune_unused();
	assert_eq!(assets.loaded_model_count(), 0);
	assert_eq!(assets.gpu_resource_count(), 0);
	drop(prepared);
	let prepared = prepare_import(&temporary, &source, "imported.glb").unwrap();
	let gpu = assets.prepare_gpu_asset(renderer, &prepared).unwrap();
	let published = prepared.publish().unwrap();
	assets.promote_gpu_asset(gpu);
	let metadata = bridge::animation_asset::inspect(&temporary, &published.id).unwrap();
	let mut scene = scene_io::load_data(&source_root.join("scenes/animation-room.json"))
		.unwrap()
		.data;
	scene.entities.retain(|e| e.key.as_str() == "clip-demo");
	scene.bindings.clear();
	scene.camera.follow = None;
	let entity = &mut scene.entities[0];
	entity.model_path = "imported.glb".into();
	entity.asset_id = Some(published.id);
	entity.animation.as_mut().unwrap().settings.source = metadata.source;
	let (first, _) = scene_io::instantiate(&scene, &mut assets, renderer).unwrap();
	// A pinned live model remains valid after external file replacement;
	// opening the same authored source in a fresh cache must reject it.
	std::fs::write(temporary.join("imported.glb"), b"changed source").unwrap();
	let (second, _) = scene_io::instantiate(&scene, &mut assets, renderer).unwrap();
	let mut fresh = AssetManager::new(&temporary).unwrap();
	assert!(scene_io::instantiate(&scene, &mut fresh, renderer).is_err());
	renderer.wait_idle().unwrap();
	drop(first);
	drop(second);
	drop(fresh);
	drop(assets);
	std::fs::remove_dir_all(temporary).unwrap();
}

fn check_animated_spawn_retirement(
	app: &mut GameApp,
	draw: fn(&mut GameApp),
	baseline: (usize, usize),
) {
	use runtime::{
		components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
		state_machine::{MachineAction, MachineDef, MachineTrigger, StateDef, TransitionDef},
	};
	let mut entity = app
		.scene
		.to_data(&app.camera)
		.unwrap()
		.entities
		.into_iter()
		.find(|e| e.key.as_str() == "clip-demo")
		.unwrap();
	entity.key = runtime::EntityKey::new("animated-copy").unwrap();
	entity.parent = None;
	entity.transform.position = [0., 0., 0.];
	entity.native_behaviors = vec![NativeBehaviorComponent {
		id: ComponentId::new("lifetime").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::StateMachine {
			definition: MachineDef {
				schema_version: 1,
				initial: "alive".into(),
				inputs: vec![],
				states: vec![
					StateDef {
						id: "alive".into(),
						on_enter: vec![],
						transitions: vec![TransitionDef {
							when: MachineTrigger::AfterTicks { ticks: 2 },
							next: "gone".into(),
						}],
					},
					StateDef {
						id: "gone".into(),
						on_enter: vec![MachineAction::DespawnOwner],
						transitions: vec![],
					},
				],
			},
		},
	}];
	let prefab = runtime::editor::PrefabData {
		schema_version: 3,
		composition: Default::default(),
		entities: vec![entity],
		gameplay: None,
		reference_slots: std::collections::BTreeMap::new(),
	};
	for _ in 0..4 {
		let mut prepared = app
			.editor
			.prepare_runtime_spawn(&prefab, &"a".repeat(64), [3., 0., 0.], None)
			.unwrap();
		let spawned = prepared.entities()[0].key.clone();
		let document = app.editor.runtime_spawn_document(&prepared).unwrap();
		let (mut candidate, _) = scene_io::instantiate(
			&document,
			app.asset_manager.as_mut().unwrap(),
			app.renderer.as_ref().unwrap(),
		)
		.unwrap();
		app.renderer.as_ref().unwrap().wait_idle().unwrap();
		prepared
			.set_visibility_capabilities(std::collections::HashSet::from([spawned.clone()]))
			.unwrap();
		app.editor.commit_runtime_spawn(prepared).unwrap();
		candidate
			.synchronize_animation(app.editor.animations().unwrap())
			.unwrap();
		app.scene = candidate;
		assert_eq!(app.editor.animations().unwrap().poses().count(), 3);
		draw(app);
		for _ in 0..8 {
			app.tick_play(1. / 60.).unwrap();
			app.input.end_frame();
			draw(app);
		}
		assert!(!app.editor.has_runtime_instances());
		assert!(app.scene.entity_by_key(&spawned).is_none());
		assert_eq!(app.editor.animations().unwrap().poses().count(), 2);
		let assets = app.asset_manager.as_ref().unwrap();
		assert_eq!(
			(assets.gpu_logical_bytes(), assets.gpu_resource_count()),
			baseline
		);
	}
	println!(
		"ANIMATION_NATIVE_RETIREMENT cycles=4 live_instances=2 palette_matrices=4 gpu_bytes={} gpu_resources={}",
		baseline.0, baseline.1
	);
}

#[path = "../../../../tests/support/project_fixture.rs"]
mod test_project;
