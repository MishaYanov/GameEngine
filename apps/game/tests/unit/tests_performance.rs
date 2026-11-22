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
#[ignore = "requires Windows Vulkan and the authored courtyard fixture"]
fn native_prepared_scene_references_preserve_scene_and_geometry() {
	use renderer::{DrawStatus, vulkan::DrawMetrics};
	use runtime::{SceneData, scene_io::PreparedSceneReferences};

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
			let path = asset_root.join("scenes/courtyard-release-learning.json");
			let authored = Arc::new(scene_io::load_data(&path).unwrap().data);
			let references = PreparedSceneReferences::build(Arc::clone(&authored), || false)
				.unwrap()
				.expect("The courtyard fits the warming budget");
			assert!(references.matches(&authored));

			let mut revised = (*authored).clone();
			let player = revised
				.entities
				.iter_mut()
				.find(|entity| entity.key.as_str() == "courtyard-player")
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
			let mut render = |document: &SceneData, references| {
				let (scene, camera) = scene_io::instantiate_with_references(
					document,
					&mut assets,
					&renderer,
					references,
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

			let cold = render(&authored, None);
			assert!(cold.metrics.draw_calls > 0);
			assert_eq!(render(&authored, Some(references)), cold);
			let revised_cold = render(&revised, None);
			assert_ne!(revised_cold.geometry, cold.geometry);
			assert_eq!(render(&revised, Some(stale)), revised_cold);
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

#[test]
#[ignore = "requires Windows Vulkan and an authored two-chapter manifest"]
fn chapter_scene_prefetch_native_probe() {
	struct PrefetchProbe {
		manifest: PathBuf,
		enabled: bool,
		completed: bool,
	}

	impl ApplicationHandler for PrefetchProbe {
		#[allow(clippy::field_reassign_with_default)]
		fn resumed(&mut self, events: &ActiveEventLoop) {
			let project = ProjectData::load(&self.manifest).unwrap();
			let asset_root = self.manifest.parent().unwrap().join("assets");
			let window = Arc::new(
				events
					.create_window(
						Window::default_attributes()
							.with_visible(false)
							.with_inner_size(winit::dpi::PhysicalSize::new(1280, 720)),
					)
					.unwrap(),
			);
			let mut app = GameApp::default();
			app.project = Some(project);
			app.project_path = Some(self.manifest.clone());
			app.installed_project = true;
			app.standalone_runtime = true;
			app.smoke_test = true;
			app.smoke_prefetch_probe = true;
			app.scene_prefetch_enabled = self.enabled;
			app.initialize_window(window, asset_root).unwrap();
			assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
			app.exercise_scene_loads(32).unwrap();
			app.wait_for_gpu().unwrap();
			self.completed = true;
			events.exit();
		}

		fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
	}

	let manifest = PathBuf::from(
		std::env::var_os("GAME_ENGINE_QA_PREFETCH_PROJECT")
			.expect("Set path to a chaptered project.json"),
	);
	let output = PathBuf::from(
		std::env::var_os("GAME_ENGINE_QA_PREFETCH_LOG").expect("Set an unused JSONL output path"),
	);
	let enabled = std::env::var("GAME_ENGINE_QA_PREFETCH_ENABLED").as_deref() == Ok("1");
	let mut config = engine_logger::Config::new("native-chapter-prefetch-benchmark");
	config.console = false;
	let _logging = engine_logger::init(config).unwrap();
	let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut probe = PrefetchProbe {
		manifest,
		enabled,
		completed: false,
	};
	event_loop.run_app(&mut probe).unwrap();
	assert!(probe.completed);

	use std::io::Write;
	let mut file = std::io::BufWriter::new(
		std::fs::OpenOptions::new()
			.create_new(true)
			.write(true)
			.open(output)
			.unwrap(),
	);
	let history = engine_logger::snapshot(engine_logger::SnapshotRequest {
		limit: 2000,
		..Default::default()
	});
	assert!(
		!history.history_gap && !history.has_more,
		"incomplete benchmark log"
	);
	for entry in history.entries {
		if matches!(entry.component.as_str(), "scene_load" | "scene_prefetch") {
			serde_json::to_writer(&mut file, &entry).unwrap();
			file.write_all(b"\n").unwrap();
		}
	}
	file.flush().unwrap();
}
