//! Opt-in native acceptance route for the authored or staged Courtyard project.
use super::*;
use std::fs;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires native Vulkan and a window server"]
#[allow(clippy::field_reassign_with_default)]
fn exported_courtyard_completes_and_restarts_in_a_hidden_native_player() {
	#[derive(Default)]
	struct RouteCheck {
		completed: bool,
	}
	impl ApplicationHandler for RouteCheck {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let manifest = std::env::var_os("GAME_ENGINE_QA_PROJECT")
				.map(PathBuf::from)
				.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.json"))
				.canonicalize()
				.unwrap();
			let project = ProjectData::load(&manifest).unwrap();
			assert_eq!(
				project.gameplay_module.as_deref(),
				Some(progress::COURTYARD_RUN_ID)
			);
			assert_eq!(project.startup_scene_id.as_str(), "courtyard");
			let assets = manifest.parent().unwrap().join("assets");
			let mut source_files = vec![(manifest.clone(), fs::read(&manifest).unwrap())];
			for scene in project.scenes.iter() {
				let path = assets.join(scene.asset_path());
				source_files.push((path.clone(), fs::read(path).unwrap()));
			}
			let startup = assets.join(project.startup_scene_path().unwrap());
			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
					)
					.unwrap(),
			);
			let size = window.inner_size();
			let renderer =
				WindowRenderer::new(Arc::clone(&window), size.width, size.height).unwrap();
			let mut loaded = scene_io::load(&startup, &assets, &renderer).unwrap();
			let mut editor = GameApp::prepare_editor(&mut loaded).unwrap();
			editor.set_initial_project(project.clone()).unwrap();
			editor
				.set_module_resolver(Arc::new(progress::resolve))
				.unwrap();
			editor.start_play(&mut loaded.scene).unwrap();
			let original = loaded.scene.to_data(&loaded.camera).unwrap();
			let mut app = GameApp::default();
			app.scene = loaded.scene;
			app.camera = loaded.camera;
			app.editor = editor;
			app.asset_manager = Some(loaded.assets);
			app.renderer = Some(renderer);
			app.window = Some(window);
			app.asset_root = Some(assets);
			app.scene_path = Some(startup);
			app.project_path = Some(manifest);
			app.project_name = Some(project.name.clone());
			app.project = Some(project);
			app.standalone_runtime = true;
			app.installed_project = true;
			app.render().unwrap();
			let dt = 1.0 / 60.0;
			for (key, count, scene, expected) in [
				(Key::A, 90, "courtyard", (0, false, false, false)),
				(Key::D, 190, "courtyard", (1, true, false, false)),
				(Key::A, 240, "finish", (1, true, true, false)),
				(Key::D, 90, "finish", (1, true, true, true)),
			] {
				app.input.reset();
				app.input.press(key);
				for _ in 0..count {
					app.tick_play(dt).unwrap();
					app.input.end_frame();
				}
				assert_eq!(app.editor.play_scene_id().unwrap().as_str(), scene);
				let progress = app
					.editor
					.play_progress::<progress::CampaignProgress>()
					.unwrap();
				assert_eq!(
					(
						progress.score,
						progress.has_courtyard_key,
						progress.courtyard_exit_reached,
						progress.completed
					),
					expected
				);
				app.render().unwrap();
			}
			let ending = app.scene.to_data(&app.camera).unwrap();
			app.input.reset();
			app.input.press(Key::A);
			app.update(dt);
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), ending);
			app.input.reset();
			app.input.press(Key::F10);
			app.update(0.0);
			assert!(!app.play_update_error_reported);
			assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
			let progress = app
				.editor
				.play_progress::<progress::CampaignProgress>()
				.unwrap();
			assert_eq!(
				(
					progress.score,
					progress.has_courtyard_key,
					progress.courtyard_exit_reached,
					progress.completed
				),
				(0, false, false, false)
			);
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), original);
			// Fresh sensors sample their initial occupancy on the first fixed step.
			assert!(app.editor.needs_tick());
			app.input.reset();
			app.tick_play(dt).unwrap();
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), original);
			assert!(!app.editor.needs_tick());
			app.render().unwrap();
			app.input.reset();
			app.input.press(Key::D);
			app.tick_play(dt).unwrap();
			let window_id = app.window.as_ref().unwrap().id();
			ApplicationHandler::window_event(
				&mut app,
				events,
				window_id,
				WindowEvent::Focused(false),
			);
			assert!(!app.input.is_held(Key::D));
			let blurred = app.scene.to_data(&app.camera).unwrap();
			app.tick_play(dt).unwrap();
			assert_eq!(
				app.scene.to_data(&app.camera).unwrap().entities,
				blurred.entities
			);
			// Input is neutral immediately; the authored follow rig may finish its
			// bounded smoothing after the actor stops, then the scheduler sleeps.
			for _ in 0..120 {
				if !app.editor.needs_tick() {
					break;
				}
				app.tick_play(dt).unwrap();
				assert_eq!(
					app.scene.to_data(&app.camera).unwrap().entities,
					blurred.entities
				);
			}
			assert!(!app.editor.needs_tick());
			let settled = app.scene.to_data(&app.camera).unwrap();
			ApplicationHandler::window_event(
				&mut app,
				events,
				window_id,
				WindowEvent::Focused(true),
			);
			app.tick_play(0.0).unwrap();
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), settled);
			for (path, bytes) in source_files {
				assert_eq!(fs::read(path).unwrap(), bytes);
			}
			app.renderer.as_ref().unwrap().wait_idle().unwrap();
			drop(app);
			self.completed = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let mut config = engine_logger::Config::new("native-exported-route-test");
	config.console = false;
	let _logging = engine_logger::init(config).unwrap();
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = RouteCheck::default();
	events.run_app(&mut check).unwrap();
	assert!(check.completed);
	assert!(
		!engine_logger::snapshot(Default::default())
			.entries
			.iter()
			.any(|entry| entry.level == engine_logger::Level::Error)
	);
}
