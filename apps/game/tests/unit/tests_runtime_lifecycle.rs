//! Native acceptance uses the same player spawn/retirement code as a real run.
use super::*;
use winit::platform::windows::EventLoopBuilderExtWindows;

#[test]
#[ignore = "requires native Vulkan and a hidden window server"]
fn component_lab_retires_spawned_resources_and_restarts_in_native_player() {
	#[derive(Default)]
	struct Check {
		complete: bool,
	}
	impl ApplicationHandler for Check {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let manifest = std::env::var_os("GAME_ENGINE_QA_COMPONENT_PROJECT")
				.map(PathBuf::from)
				.unwrap_or_else(|| {
					PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("component-demo.json")
				})
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
			let mut loaded = scene_io::load(&startup, &root, &renderer).unwrap();
			let mut editor =
				SceneEditor::for_project(&loaded.scene, &loaded.camera, project.clone()).unwrap();
			editor.start_play(&mut loaded.scene).unwrap();
			let original = loaded
				.scene
				.to_data(&editor.render_camera().unwrap())
				.unwrap();
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
			let baseline = {
				let assets = app.asset_manager.as_ref().unwrap();
				(
					assets.gpu_logical_bytes(),
					assets.gpu_resource_count(),
					assets.loaded_model_count(),
				)
			};
			// Stand at the emitter. Each admission is followed by contact removal.
			app.input.press(Key::W);
			for _ in 0..45 {
				app.tick_play(1.0 / 60.0).unwrap();
				app.input.end_frame();
			}
			app.input.reset();
			let mut admissions = 0;
			for _ in 0..1200 {
				app.tick_play(1.0 / 60.0).unwrap();
				if app.editor.has_runtime_instances() {
					admissions += 1;
					app.render().unwrap();
				} else {
					let assets = app.asset_manager.as_ref().unwrap();
					assert_eq!(
						(
							assets.gpu_logical_bytes(),
							assets.gpu_resource_count(),
							assets.loaded_model_count()
						),
						baseline
					);
				}
			}
			assert!(admissions >= 8, "The emitter must admit multiple instances");
			// A scene install retires the last pending instance and native state.
			app.switch_player_scene(SceneId::new("component-rest").unwrap())
				.unwrap();
			assert!(!app.editor.has_runtime_instances());
			assert!(app.editor.pending_runtime_spawns().is_empty());
			app.restart_player_game().unwrap();
			assert!(!app.editor.has_runtime_instances());
			assert_eq!(app.scene.to_data(&app.camera).unwrap(), original);
			let assets = app.asset_manager.as_ref().unwrap();
			assert_eq!(
				(
					assets.gpu_logical_bytes(),
					assets.gpu_resource_count(),
					assets.loaded_model_count()
				),
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
}
