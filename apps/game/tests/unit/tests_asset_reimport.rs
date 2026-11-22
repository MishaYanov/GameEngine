//! Opt-in proof that prepared GPU assets preserve and then replace live Arcs.
use super::*;
use runtime::assets::project_assets::{prepare_import, prepare_move, prepare_reimport};
use std::{collections::HashSet, fs};
use winit::platform::windows::EventLoopBuilderExtWindows;

struct ToggleVisibility;
impl runtime::simulation_session::GameModuleFactory for ToggleVisibility {
	fn create(
		&self,
		_: &ProjectData,
		_: &SceneId,
	) -> Result<Box<dyn runtime::simulation_session::GameModule>, String> {
		Ok(Box::new(ToggleVisibility))
	}
}
impl runtime::simulation_session::GameModule for ToggleVisibility {
	fn needs_tick(&self, _: &runtime::simulation_session::InputFrame) -> bool {
		true
	}
	fn plan(
		&self,
		world: &runtime::world::RuntimeWorld,
		_: &runtime::simulation_session::InputFrame,
		_: f32,
		output: &mut runtime::simulation_session::ActionWriter<'_>,
	) -> Result<(), String> {
		let entity = &world.data().entities[0];
		output.push(runtime::simulation_session::SimulationAction::SetVisible {
			entity: entity.key.clone(),
			visible: !entity.renderable,
		})
	}
}

fn check_runtime_visibility(assets: &mut AssetManager, renderer: &WindowRenderer) {
	let mut data =
		runtime::SceneData::from_json(include_str!("../../assets/scenes/robots.json")).unwrap();
	data.entities = vec![
		runtime::scene_data::EntityData::new(
			runtime::EntityKey::new("hidden").unwrap(),
			"Hidden model",
			"moved.glb",
		)
		.with_renderable(false),
	];
	data.bindings.clear();
	data.default_game_camera = None;
	data.camera.follow = None;
	for missing in [false, true] {
		if missing {
			data.entities[0].model_path = "missing-marker.glb".into();
		}
		let (mut scene, camera) = scene_io::instantiate(&data, assets, renderer).unwrap();
		let id = scene.entity_by_key(&data.entities[0].key).unwrap();
		assert!(!scene.is_renderable(id));
		let project = ProjectData::for_scene("Visibility", "main.json", &data).unwrap();
		let mut editor = SceneEditor::for_project(&scene, &camera, project).unwrap();
		editor
			.set_module_factory(Arc::new(ToggleVisibility))
			.unwrap();
		editor.start_play(&mut scene).unwrap();
		let result = editor.update_play(&mut scene, &InputState::default(), 1.0 / 60.0);
		if missing {
			assert!(
				result.is_err(),
				"missing native model must fail inside the CPU tick transaction"
			);
			assert!(!scene.is_renderable(id));
		} else {
			result.unwrap();
			assert!(scene.is_renderable(id));
			assert!(!scene.try_render_objects().unwrap().is_empty());
			editor
				.update_play(&mut scene, &InputState::default(), 1.0 / 60.0)
				.unwrap();
			assert!(!scene.is_renderable(id));
		}
		editor.stop_play(&mut scene).unwrap();
		assert_eq!(scene.to_data(&camera).unwrap(), data);
	}
}

#[test]
#[ignore = "requires native Vulkan and a window server"]
fn native_asset_reimport_promotes_candidates_only_after_successful_publication() {
	#[derive(Default)]
	struct Check {
		completed: bool,
	}
	impl ApplicationHandler for Check {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let root = std::env::temp_dir().join(format!(
				"native-reimport-{}",
				runtime::asset_catalog::AssetId::generate().as_str()
			));
			fs::create_dir(&root).unwrap();
			let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
				.join("assets/props/cube.glb")
				.canonicalize()
				.unwrap();
			let asset = prepare_import(&root, &source, "cube.glb")
				.unwrap()
				.publish()
				.unwrap();
			let reference = runtime::asset_registry::AssetRef::new("cube.glb", Some(0))
				.unwrap()
				.with_asset_id(Some(asset.id.clone()));
			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(128, 128)),
					)
					.unwrap(),
			);
			let renderer = WindowRenderer::new(window, 128, 128).unwrap();
			let mut assets = AssetManager::new(&root).unwrap();
			let old = assets.load_asset(&renderer, &reference).unwrap();
			let original_bytes = assets.gpu_logical_bytes();
			let used = HashSet::from([0]);
			let catalog_bytes = fs::read(root.join(runtime::asset_catalog::CATALOG_FILE)).unwrap();
			let file_bytes = fs::read(root.join("cube.glb")).unwrap();
			assets
				.set_gpu_budget(original_bytes, assets.gpu_resource_count())
				.unwrap();
			let over_budget =
				prepare_reimport(&root, &asset.id, Some(&source), &asset.fingerprint, &used)
					.unwrap();
			assert!(assets.prepare_gpu_asset(&renderer, &over_budget).is_err());
			assert_eq!(assets.gpu_logical_bytes(), original_bytes);
			assert_eq!(
				fs::read(root.join(runtime::asset_catalog::CATALOG_FILE)).unwrap(),
				catalog_bytes
			);
			assert_eq!(fs::read(root.join("cube.glb")).unwrap(), file_bytes);
			assert!(Arc::ptr_eq(
				&assets.load_asset(&renderer, &reference).unwrap(),
				&old
			));
			drop(over_budget);
			assets
				.set_gpu_budget(
					AssetManager::GPU_LOGICAL_BYTE_LIMIT,
					AssetManager::GPU_RESOURCE_LIMIT,
				)
				.unwrap();
			let rejected =
				prepare_reimport(&root, &asset.id, Some(&source), &asset.fingerprint, &used)
					.unwrap();
			let candidate = assets.prepare_gpu_asset(&renderer, &rejected).unwrap();
			assert!(
				assets.gpu_logical_bytes() > original_bytes,
				"candidate and current allocation both count"
			);
			assert!(Arc::ptr_eq(
				&assets.load_asset(&renderer, &reference).unwrap(),
				&old
			));
			fs::write(rejected.candidate_path(), b"changed after GPU preparation").unwrap();
			assert!(rejected.publish().is_err());
			assets.discard_gpu_asset(candidate);
			assert_eq!(assets.gpu_logical_bytes(), original_bytes);
			assert!(Arc::ptr_eq(
				&assets.load_asset(&renderer, &reference).unwrap(),
				&old
			));
			let accepted =
				prepare_reimport(&root, &asset.id, Some(&source), &asset.fingerprint, &used)
					.unwrap();
			let candidate = assets.prepare_gpu_asset(&renderer, &accepted).unwrap();
			accepted.publish().unwrap();
			renderer.wait_idle().unwrap();
			assets.promote_gpu_asset(candidate);
			let replacement = assets.load_asset(&renderer, &reference).unwrap();
			assert!(!Arc::ptr_eq(&old, &replacement));
			assert_eq!(assets.loaded_model_count(), 1);
			assert!(
				assets.gpu_logical_bytes() > original_bytes,
				"externally held old scene is still charged"
			);
			assert!(Arc::ptr_eq(
				&assets.load_asset(&renderer, &reference).unwrap(),
				&replacement
			));
			drop(old);
			assets.prune_unused();
			assert_eq!(assets.gpu_logical_bytes(), original_bytes);
			let moved = prepare_move(&root, &asset.id, "moved.glb").unwrap();
			let candidate = assets.prepare_gpu_asset(&renderer, &moved).unwrap();
			moved.publish().unwrap();
			assets.promote_gpu_asset(candidate);
			let moved_model = assets.load_asset(&renderer, &reference).unwrap();
			assert!(
				Arc::ptr_eq(&moved_model, &replacement),
				"moving unchanged bytes reuses existing GPU resources"
			);
			assert!(
				Arc::ptr_eq(
					&assets
						.load_asset(
							&renderer,
							&runtime::asset_registry::AssetRef::new("cube.glb", Some(0)).unwrap()
						)
						.unwrap(),
					&moved_model
				),
				"legacy alias and stable identity share the promoted GPU model"
			);
			assert!(!root.join("cube.glb").exists());
			drop(replacement);
			assets.prune_unused();
			assert_eq!(assets.loaded_model_count(), 1);
			assert_eq!(assets.gpu_logical_bytes(), original_bytes);
			check_runtime_visibility(&mut assets, &renderer);
			renderer.wait_idle().unwrap();
			drop(moved_model);
			drop(assets);
			drop(renderer);
			fs::remove_dir_all(root).unwrap();
			self.completed = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let mut config = engine_logger::Config::new("native-reimport-test");
	config.console = false;
	let _logging = engine_logger::init(config).unwrap();
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut check = Check::default();
	events.run_app(&mut check).unwrap();
	assert!(check.completed);
	assert!(
		!engine_logger::snapshot(Default::default())
			.entries
			.iter()
			.any(|entry| entry.level == engine_logger::Level::Error)
	);
}
