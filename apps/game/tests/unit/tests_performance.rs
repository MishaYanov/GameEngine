//! Opt-in measurements on the same authored fixture used by the CPU baseline.
use super::*;
use std::os::windows::process::CommandExt;
use winit::platform::windows::EventLoopBuilderExtWindows;

// Sample outside timed frames. CREATE_NO_WINDOW keeps the read-only process
// counter helper hidden; the numeric PID is this benchmark's own process.
pub(super) fn working_set() -> (String, String) {
	let query = format!(
		"$p=Get-Process -Id {}; $p.WorkingSet64; $p.PeakWorkingSet64",
		std::process::id()
	);
	let output = std::process::Command::new("powershell.exe")
		.args([
			"-NoLogo",
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			&query,
		])
		.creation_flags(0x08000000)
		.output();
	let values = output
		.ok()
		.filter(|output| output.status.success())
		.map(|output| {
			String::from_utf8_lossy(&output.stdout)
				.lines()
				.filter_map(|line| line.trim().parse::<u64>().ok())
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	if values.len() == 2 {
		(values[0].to_string(), values[1].to_string())
	} else {
		("null".into(), "null".into())
	}
}

#[test]
#[ignore = "requires native Vulkan and the representative performance fixture"]
fn representative_native_scene_performance_baseline() {
	fn percentile(samples: &mut [f64], fraction: f64) -> f64 {
		samples.sort_by(f64::total_cmp);
		samples[(samples.len() as f64 * fraction).ceil() as usize - 1]
	}
	#[derive(Default)]
	struct Benchmark {
		completed: bool,
	}
	impl ApplicationHandler for Benchmark {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
			let scene_path = std::env::var_os("ENGINE_PERF_SCENE")
				.map(PathBuf::from)
				.unwrap_or_else(|| root.join("target/performance/representative-scene.json"));
			let assets = std::env::var_os("ENGINE_PERF_ASSETS")
				.map(PathBuf::from)
				.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(1280, 720)),
					)
					.unwrap(),
			);
			let size = window.inner_size();
			let mut renderer = WindowRenderer::new(window, size.width, size.height).unwrap();
			let load = Instant::now();
			let loaded = scene_io::load(&scene_path, &assets, &renderer).unwrap();
			let load_ms = load.elapsed().as_secs_f64() * 1000.0;
			renderer
				.set_lights(&loaded.scene.renderer_lights())
				.unwrap();
			let mut measured = Vec::new();
			let mut gpu = Vec::new();
			let mut after_warm = None;
			let mut last_visible = 0;
			for frame in 0..140 {
				let start = Instant::now();
				let [width, height] = renderer
					.prepare()
					.unwrap()
					.expect("Hidden benchmark swapchain must have a nonzero extent");
				let uniform = CameraUniform::perspective(
					loaded.camera.position,
					loaded.camera.target(),
					[0.0, 1.0, 0.0],
					loaded.camera.vertical_fov,
					width as f32 / height as f32,
					loaded.camera.near,
					loaded.camera.far,
				);
				let frustum = runtime::visibility::Frustum::from_view_projection(
					glam::Mat4::from_cols_array(&uniform.view_projection),
				)
				.unwrap();
				let (objects, stats) = loaded.scene.try_render_visible(Some(&frustum)).unwrap();
				last_visible = stats.visible_entities;
				renderer.draw(&uniform, &objects).unwrap();
				if frame == 19 {
					after_warm = renderer
						.resource_metrics()
						.map(|metrics| metrics.totals.created_resources);
				}
				if frame >= 20 {
					measured.push(start.elapsed().as_secs_f64() * 1000.0);
					if let Some(ms) = renderer.last_metrics().gpu_time_ms {
						gpu.push(ms);
					}
				}
			}
			renderer.wait_idle().unwrap();
			let metrics = renderer
				.resource_metrics()
				.expect("Renderer resource counters must be available");
			assert_eq!(
				after_warm,
				Some(metrics.totals.created_resources),
				"Steady frames must reuse GPU allocations"
			);
			let gpu_median = if gpu.is_empty() {
				"null".into()
			} else {
				percentile(&mut gpu, 0.5).to_string()
			};
			let gpu_p95 = if gpu.is_empty() {
				"null".into()
			} else {
				percentile(&mut gpu, 0.95).to_string()
			};
			let (working_set, peak_working_set) = working_set();
			let report = format!(
				r#"{{"schema_version":1,"scene_load_wall_ms":{load_ms},"warm_frames":20,"measured_frames":120,"frame_submission_wall_median_ms":{},"frame_submission_wall_p95_ms":{},"gpu_timestamp_median_ms":{gpu_median},"gpu_timestamp_p95_ms":{gpu_p95},"gpu_timestamps_supported":{},"visible_entities":{last_visible},"cached_models":{},"gpu_live_resources":{},"gpu_created_resources":{},"gpu_tracked_allocation_bytes":{},"asset_gpu_logical_bytes":{},"steady_gpu_allocation_growth":0,"process_working_set_bytes":{working_set},"peak_process_working_set_bytes":{peak_working_set},"timing_scope":"frame wall time includes swapchain waits, culling, and submission; GPU values use device timestamps"}}"#,
				percentile(&mut measured, 0.5),
				percentile(&mut measured, 0.95),
				renderer.last_metrics().gpu_timestamps_supported,
				loaded.assets.loaded_model_count(),
				metrics.totals.live_resources,
				metrics.totals.created_resources,
				metrics.totals.allocation_bytes,
				loaded.assets.gpu_logical_bytes()
			);
			let output = std::env::var_os("ENGINE_PERF_OUTPUT")
				.map(PathBuf::from)
				.unwrap_or_else(|| root.join("target/performance/native.json"));
			std::fs::create_dir_all(output.parent().unwrap()).unwrap();
			std::fs::write(&output, &report).unwrap();
			eprintln!(
				"Native performance fixture {}\nReport {}\n{report}",
				scene_path.display(),
				output.display()
			);
			drop(loaded);
			drop(renderer);
			self.completed = true;
			events.exit();
		}
		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}
	let mut config = engine_logger::Config::new("native-performance-baseline");
	config.console = false;
	let _logging = engine_logger::init(config).unwrap();
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut benchmark = Benchmark::default();
	events.run_app(&mut benchmark).unwrap();
	assert!(benchmark.completed);
	assert!(
		!engine_logger::snapshot(Default::default())
			.entries
			.iter()
			.any(|entry| entry.level == engine_logger::Level::Error)
	);
}

#[test]
#[ignore = "requires Windows Vulkan and the camera room scene fixture"]
fn native_prepared_scene_references_preserve_scene_and_geometry() {
	use renderer::{DrawStatus, vulkan::DrawMetrics};
	use runtime::{
		SceneData,
		asset_catalog::{
			AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MeshSignature, PersistentAsset,
		},
		assets::{PreparedModelBatch, project_assets},
		scene_data::EntityData,
		scene_io::PreparedSceneReferences,
	};
	use std::fs;

	struct ManagedFixture(PathBuf);
	impl ManagedFixture {
		fn new(asset_root: &std::path::Path) -> Self {
			let target = asset_root.join("../../../target");
			fs::create_dir_all(&target).unwrap();
			let path = target.join(format!("warm-vulkan-{}", AssetId::generate().as_str()));
			fs::create_dir(&path).unwrap();
			Self(path)
		}
	}
	impl Drop for ManagedFixture {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}

	#[derive(Debug, PartialEq)]
	struct GeometryPart {
		index_count: u32,
		model: [f32; 16],
		tint: [f32; 4],
		flags: [u32; 4],
	}

	#[derive(Debug, PartialEq)]
	struct RenderedScene {
		scene: SceneData,
		geometry: Vec<GeometryPart>,
		metrics: DrawMetrics,
	}

	#[derive(Default)]
	struct ReferenceProbe {
		completed: bool,
	}

	impl ApplicationHandler for ReferenceProbe {
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let asset_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
			let path = asset_root.join("scenes/camera-room.json");
			let authored = Arc::new(scene_io::load_data(&path).unwrap().data);
			let references = PreparedSceneReferences::build(Arc::clone(&authored), || false)
				.unwrap()
				.expect("The camera room fits the warming budget");
			assert!(references.matches(&authored));

			let mut revised = (*authored).clone();
			let player = revised
				.entities
				.iter_mut()
				.find(|entity| entity.key.as_str() == "player-visual")
				.unwrap();
			assert_eq!(player.model_path, "props/capsule.glb");
			player.model_path = "props/cube.glb".into();
			let revised = Arc::new(revised);
			let stale = PreparedSceneReferences::build(Arc::clone(&authored), || false)
				.unwrap()
				.unwrap();
			assert!(!stale.matches(&revised));

			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(1280, 720)),
					)
					.unwrap(),
			);
			let size = window.inner_size();
			let mut renderer = WindowRenderer::new(window, size.width, size.height).unwrap();
			let mut assets = AssetManager::new(&asset_root).unwrap();
			let mut render = |document: &SceneData,
			                  assets: &mut AssetManager,
			                  references,
			                  models: Option<&mut PreparedModelBatch>| {
				let (scene, camera) = scene_io::instantiate_with_warming(
					document,
					assets,
					&renderer,
					references,
					models,
					|_, _| {},
					|| false,
				)
				.unwrap()
				.expect("Uncancelled construction should produce a scene");
				renderer.set_lights(&scene.renderer_lights()).unwrap();
				let [width, height] = renderer.prepare().unwrap().unwrap();
				let camera_uniform = CameraUniform::perspective(
					camera.position,
					camera.target(),
					[0.0, 1.0, 0.0],
					camera.vertical_fov,
					width as f32 / height as f32,
					camera.near,
					camera.far,
				);
				let objects = scene.try_render_objects().unwrap();
				assert_eq!(
					renderer.draw(&camera_uniform, &objects).unwrap(),
					DrawStatus::Rendered
				);
				renderer.wait_idle().unwrap();
				RenderedScene {
					scene: scene.to_data(&camera).unwrap(),
					geometry: objects
						.iter()
						.map(|object| GeometryPart {
							index_count: object.mesh().index_count(),
							model: object.transform().model,
							tint: object.transform().base_color_factor,
							flags: object.transform().flags,
						})
						.collect(),
					metrics: DrawMetrics {
						gpu_time_ms: None,
						..renderer.last_metrics()
					},
				}
			};

			let cold = render(&authored, &mut assets, None, None);
			assert!(cold.metrics.draw_calls > 0);
			assert_eq!(render(&authored, &mut assets, Some(references), None), cold);
			let revised_cold = render(&revised, &mut assets, None, None);
			assert_ne!(revised_cold.geometry, cold.geometry);
			assert_eq!(
				render(&revised, &mut assets, Some(stale), None),
				revised_cold
			);

			let fixture = ManagedFixture::new(&asset_root);
			let model = fixture.0.join("managed.glb");
			let catalog_file = fixture.0.join(CATALOG_FILE);
			let bytes = fs::read(asset_root.join("props/cube.glb")).unwrap();
			let id = AssetId::generate();
			let mut catalog = AssetCatalog {
				schema_version: 1,
				assets: vec![PersistentAsset {
					id: id.clone(),
					path: "managed.glb".into(),
					kind: AssetKind::Model,
					aliases: vec![],
					revision: 1,
					fingerprint: project_assets::fingerprint(&bytes),
					bytes: bytes.len() as u64,
					source_path: None,
					dependencies: vec![],
					meshes: vec![MeshSignature {
						index: 0,
						name: "Managed".into(),
						primitives: 1,
					}],
				}],
			};
			fs::write(&model, bytes).unwrap();
			fs::write(&catalog_file, catalog.to_json().unwrap()).unwrap();
			let mut document = (*authored).clone();
			document.bindings.clear();
			document.default_game_camera = None;
			document.camera.follow = None;
			document.camera.orbit = None;
			document.entities = vec![
				EntityData::new(
					runtime::EntityKey::new("warm-managed").unwrap(),
					"Managed",
					"managed.glb",
				)
				.with_asset_id(Some(id)),
			];
			let document = Arc::new(document);
			let prepare = || {
				let references = PreparedSceneReferences::build(Arc::clone(&document), || false)
					.unwrap()
					.unwrap();
				let models = PreparedModelBatch::prepare(
					&fixture.0,
					references.static_model_assets(),
					&|| false,
				)
				.unwrap();
				(references, models)
			};
			let (references, mut warm) = prepare();
			let mut warmed_assets = AssetManager::new(&fixture.0).unwrap();
			let warm_scene = render(
				&document,
				&mut warmed_assets,
				Some(references),
				Some(&mut warm),
			);
			assert_eq!(warm.consumed_count(), 1);
			let mut cold_assets = AssetManager::new(&fixture.0).unwrap();
			assert_eq!(render(&document, &mut cold_assets, None, None), warm_scene);

			let (references, mut cached) = prepare();
			assert_eq!(
				render(
					&document,
					&mut warmed_assets,
					Some(references),
					Some(&mut cached)
				),
				warm_scene
			);
			assert_eq!(
				cached.consumed_count(),
				0,
				"a GPU cache hit should skip a prepared model"
			);

			let (references, mut stale_model) = prepare();
			let replacement = fs::read(asset_root.join("props/capsule.glb")).unwrap();
			let record = &mut catalog.assets[0];
			record.revision = 2;
			record.fingerprint = project_assets::fingerprint(&replacement);
			record.bytes = replacement.len() as u64;
			fs::write(&model, replacement).unwrap();
			fs::write(&catalog_file, catalog.to_json().unwrap()).unwrap();
			let mut reimported_assets = AssetManager::new(&fixture.0).unwrap();
			let reimported = render(
				&document,
				&mut reimported_assets,
				Some(references),
				Some(&mut stale_model),
			);
			assert_eq!(stale_model.consumed_count(), 0);
			assert_ne!(reimported.geometry, warm_scene.geometry);
			eprintln!(
				"NATIVE_WARM_MODEL_OK cold_triangles={} reimported_triangles={} adopted={} gpu_cache_adopted={}",
				warm_scene.metrics.triangles,
				reimported.metrics.triangles,
				warm.consumed_count(),
				cached.consumed_count(),
			);
			eprintln!(
				"NATIVE_PREPARED_SCENE_REFERENCES_OK authored_entities={} render_parts={} cold_triangles={} revised_triangles={}",
				cold.scene.entities.len(),
				cold.geometry.len(),
				cold.metrics.triangles,
				revised_cold.metrics.triangles,
			);
			self.completed = true;
			events.exit();
		}

		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}

	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut probe = ReferenceProbe::default();
	events.run_app(&mut probe).unwrap();
	assert!(probe.completed);
}
