//! Run with cargo run -p game --example scene_io_smoke (requires native Vulkan).
use renderer::{
	DrawStatus, WindowRenderer,
	vulkan::{CameraUniform, ModelPushConstants},
};
use runtime::{
	SceneData,
	scene_io::{self, LoadedScene},
};
use std::{
	fs,
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use winit::{
	application::ApplicationHandler,
	event::WindowEvent,
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	window::{Window, WindowId},
};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
	fn new() -> Self {
		let nonce = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_nanos();
		// Keep temporary scenes inside the writable checkout. The Windows
		// sandbox's redirected OS Temp directory can disallow replacement.
		let temporary_root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.ancestors()
			.nth(2)
			.unwrap()
			.join("target");
		let path = temporary_root.join(format!(
			"game-engine-scene-io-smoke-{}-{nonce}",
			std::process::id()
		));
		// Exclusive creation ensures cleanup only removes this run's directory.
		fs::create_dir(&path).unwrap();
		Self(path)
	}

	fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for TemporaryDirectory {
	fn drop(&mut self) {
		fs::remove_dir_all(&self.0).unwrap();
	}
}

fn snapshot(loaded: &LoadedScene) -> String {
	loaded
		.scene
		.to_data(&loaded.camera)
		.unwrap()
		.to_json()
		.unwrap()
}

fn assert_loaded_robots(loaded: &LoadedScene) {
	assert_eq!(loaded.entity_ids.len(), 2);
	assert_eq!(loaded.scene.instances().count(), 2);
	assert_ne!(loaded.entity_ids[0], loaded.entity_ids[1]);
	assert_eq!(loaded.scene.name(loaded.entity_ids[0]), Some("Left Robot"));
	assert_eq!(loaded.scene.name(loaded.entity_ids[1]), Some("Right Robot"));
	let data = loaded.scene.to_data(&loaded.camera).unwrap();
	assert_eq!(
		data.schema_version,
		runtime::scene_data::CURRENT_SCHEMA_VERSION
	);
	assert_eq!(
		loaded.scene.binding("robot_a"),
		loaded.scene.entity_by_key(&data.bindings["robot_a"])
	);
	assert_eq!(
		loaded.scene.binding("robot_b"),
		loaded.scene.entity_by_key(&data.bindings["robot_b"])
	);
	assert_ne!(
		loaded.scene.binding("robot_a"),
		loaded.scene.binding("robot_b")
	);
	assert_eq!(data.entities.len(), 2);
	for entity in &data.entities {
		assert_eq!(entity.model_path, "props/robot.glb");
	}

	let mut instances = loaded.scene.instances();
	let first = instances.next().unwrap();
	let second = instances.next().unwrap();
	assert!(std::ptr::eq(first.model(), second.model()));
	let parts = first.model().parts();
	assert!(!parts.is_empty(), "loaded robot must have GPU-backed parts");
	let objects = loaded.scene.render_objects();
	assert_eq!(objects.len(), parts.len() * 2);
	for (index, object) in objects.iter().enumerate() {
		let part = &parts[index % parts.len()];
		assert!(std::ptr::eq(object.mesh(), part.mesh()));
		assert!(std::ptr::eq(object.material(), part.material()));
		assert!(Arc::ptr_eq(
			object.material().texture_arc(),
			part.material().texture_arc()
		));
		let transform = if index < parts.len() {
			first.transform()
		} else {
			second.transform()
		};
		let expected =
			ModelPushConstants::from_3d(transform.position, transform.rotation, transform.scale);
		assert_eq!(object.transform().model, expected.model);
	}
}

#[derive(Default)]
struct Smoke {
	// LoadedScene releases scene instances before its asset cache. Both active
	// and retired uploaded resources must be released before the renderer.
	active: Option<LoadedScene>,
	retired: Option<LoadedScene>,
	renderer: Option<WindowRenderer>,
	window: Option<Arc<Window>>,
	temporary: Option<TemporaryDirectory>,
	asset_root: PathBuf,
	bundled_path: PathBuf,
	bundled_source: String,
	frames: usize,
	started: Option<Instant>,
}

impl Smoke {
	fn save_and_load_candidate(&mut self) {
		let active = self.active.as_mut().unwrap();
		let first = active.scene.get_mut(active.entity_ids[0]).unwrap();
		first.transform_mut().position = [-0.4, 0.6, -0.2];
		first.transform_mut().rotation = [0.2, 0.3, 0.4];
		first.transform_mut().scale = [0.5, 0.6, 0.7];
		active.camera.position = [0.2, 0.3, 4.0];
		active.camera.yaw = 0.1;
		active.camera.pitch = -0.1;
		active.camera.vertical_fov = 0.9;
		active.camera.near = 0.2;
		active.camera.far = 250.0;
		let expected = snapshot(active);
		let path = self.temporary.as_ref().unwrap().join("saved.json");
		// Verify replacement of an existing destination, including on Windows.
		fs::write(&path, &self.bundled_source).unwrap();
		scene_io::save(&path, &active.scene, &active.camera).unwrap();
		let written = SceneData::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
		assert_eq!(written.to_json().unwrap(), expected);
		assert_eq!(
			fs::read_dir(&self.temporary.as_ref().unwrap().0)
				.unwrap()
				.count(),
			1,
			"successful save must not leave a sibling temporary file"
		);

		// Live state diverges after saving; reload must restore saved values.
		active
			.scene
			.get_mut(active.entity_ids[0])
			.unwrap()
			.transform_mut()
			.position = [1.0, -1.0, -1.0];
		active.camera.position = [0.0, 0.0, 8.0];
		assert_ne!(snapshot(active), expected);
		let candidate =
			scene_io::load(&path, &self.asset_root, self.renderer.as_ref().unwrap()).unwrap();
		assert_eq!(
			candidate.source_schema_version,
			runtime::scene_data::CURRENT_SCHEMA_VERSION
		);
		assert_loaded_robots(&candidate);
		assert_eq!(snapshot(&candidate), expected);
		// Retain old uploaded resources until after drawing the candidate and
		// waiting for the GPU, rather than releasing in-flight old resources.
		self.retired = self.active.replace(candidate);
	}

	fn assert_failed_candidates_preserve_active(&self) {
		let active = self.active.as_ref().unwrap();
		let expected = snapshot(active);
		let renderer = self.renderer.as_ref().unwrap();
		let directory = self.temporary.as_ref().unwrap();
		let invalid_path = directory.join("invalid.json");
		fs::write(&invalid_path, "{ invalid JSON").unwrap();
		let error = scene_io::load(&invalid_path, &self.asset_root, renderer)
			.err()
			.expect("malformed scene should fail");
		assert!(error.contains("invalid.json"), "error context: {error}");
		assert_eq!(snapshot(active), expected);

		let mut missing = SceneData::from_json(&expected).unwrap();
		// The first entity uploads successfully before loading entity 1 fails.
		missing.entities[1].model_path = "props/scene-io-smoke-missing.glb".into();
		let missing_path = directory.join("missing-asset.json");
		fs::write(&missing_path, missing.to_json().unwrap()).unwrap();
		let error = scene_io::load(&missing_path, &self.asset_root, renderer)
			.err()
			.expect("missing second model should fail after partial upload");
		assert!(error.contains("entity 1"), "error context: {error}");
		assert!(
			error.contains("scene-io-smoke-missing.glb"),
			"error context: {error}"
		);
		assert_eq!(snapshot(active), expected);
		assert_loaded_robots(active);
	}
}

impl ApplicationHandler for Smoke {
	fn resumed(&mut self, events: &ActiveEventLoop) {
		assert!(self.renderer.is_none(), "unexpected second resume");
		self.started = Some(Instant::now());
		let window = Arc::new(
			events
				.create_window(
					Window::default_attributes()
						.with_title("Scene IO lifecycle smoke test")
						.with_visible(false)
						.with_inner_size(winit::dpi::PhysicalSize::new(320, 240)),
				)
				.unwrap(),
		);
		let size = window.inner_size();
		let renderer = WindowRenderer::new(Arc::clone(&window), size.width, size.height).unwrap();
		self.asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
		self.bundled_path = self.asset_root.join("scenes/robots.json");
		self.bundled_source = fs::read_to_string(&self.bundled_path).unwrap();
		let loaded = scene_io::load(&self.bundled_path, &self.asset_root, &renderer).unwrap();
		assert_eq!(
			loaded.source_schema_version,
			runtime::scene_data::parse_json(&self.bundled_source)
				.unwrap()
				.source_schema_version
		);
		assert_loaded_robots(&loaded);
		let expected = SceneData::from_json(&self.bundled_source).unwrap();
		assert_eq!(snapshot(&loaded), expected.to_json().unwrap());
		self.active = Some(loaded);
		self.renderer = Some(renderer);
		self.window = Some(window);
		self.temporary = Some(TemporaryDirectory::new());
	}

	fn window_event(&mut self, _events: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
		if let WindowEvent::Resized(size) = event
			&& let Some(renderer) = self.renderer.as_mut()
		{
			renderer.resize(size.width, size.height);
		}
	}

	fn about_to_wait(&mut self, events: &ActiveEventLoop) {
		if self.frames >= 3 {
			return;
		}
		assert!(
			self.started.unwrap().elapsed() < Duration::from_secs(30),
			"smoke timed out"
		);
		let renderer = self.renderer.as_mut().unwrap();
		let Some([width, height]) = renderer.prepare().unwrap() else {
			return;
		};
		let active = self.active.as_ref().unwrap();
		let camera = &active.camera;
		let uniform = CameraUniform::perspective(
			camera.position,
			camera.target(),
			[0.0, 1.0, 0.0],
			camera.vertical_fov,
			width as f32 / height as f32,
			camera.near,
			camera.far,
		);
		let objects = active.scene.render_objects();
		if renderer.draw(&uniform, &objects).unwrap() != DrawStatus::Rendered {
			return;
		}
		drop(objects);
		self.frames += 1;
		match self.frames {
			1 => self.save_and_load_candidate(),
			2 => {
				self.renderer.as_ref().unwrap().wait_idle().unwrap();
				self.retired = None;
				self.assert_failed_candidates_preserve_active();
			}
			3 => {
				self.renderer.as_ref().unwrap().wait_idle().unwrap();
				self.active = None;
				self.retired = None;
				self.renderer = None;
				self.window = None;
				assert_eq!(
					fs::read_to_string(&self.bundled_path).unwrap(),
					self.bundled_source
				);
				self.temporary = None;
				events.exit();
			}
			_ => unreachable!(),
		}
	}
}

impl Drop for Smoke {
	fn drop(&mut self) {
		if let Some(renderer) = &self.renderer {
			let _ = renderer.wait_idle();
		}
	}
}

fn main() {
	let mut config = engine_logger::Config::new("scene-io-smoke");
	config.error_backtraces = false;
	config.console = false;
	let _logging = engine_logger::init(config).unwrap();
	let events = EventLoop::new().unwrap();
	events.set_control_flow(ControlFlow::Poll);
	let mut smoke = Smoke::default();
	events.run_app(&mut smoke).unwrap();
	assert_eq!(smoke.frames, 3);
	let errors: Vec<_> = engine_logger::snapshot(Default::default())
		.entries
		.into_iter()
		.filter(|entry| entry.level == engine_logger::Level::Error)
		.collect();
	assert!(
		errors.is_empty(),
		"native validation logged errors: {errors:?}"
	);
	println!(
		"PASS: native scene load, shared models and textures, existing-file save, live transform/camera restore, ordered source paths, candidate rendering, invalid/partial-load rollback, resource retirement and shutdown"
	);
}
