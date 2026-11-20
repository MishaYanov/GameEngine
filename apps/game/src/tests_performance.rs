//! Opt-in measurements on the same authored fixture used by the CPU baseline.
use super::*;
use std::os::windows::process::CommandExt;
use winit::platform::windows::EventLoopBuilderExtWindows;

// Sample outside timed frames. CREATE_NO_WINDOW keeps the read-only process
// counter helper hidden; the numeric PID is this benchmark's own process.
fn working_set() -> (String, String) {
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
