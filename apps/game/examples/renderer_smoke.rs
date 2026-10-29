//! Run with cargo run -p game --example renderer_smoke (requires native Vulkan).
use runtime::{Transform, assets, model_instance::ModelInstance, scene::Scene};

use renderer::{
	DrawStatus, WindowRenderer,
	vulkan::{CameraUniform, Light, ModelPushConstants},
};
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
use winit::{
	application::ApplicationHandler,
	event::WindowEvent,
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	window::{Window, WindowId},
};

#[derive(Default)]
struct Smoke {
	// Drop uploaded resources before the renderer.
	scene: Scene,
	assets: Option<assets::AssetManager>,
	renderer: Option<WindowRenderer>,
	window: Option<Arc<Window>>,
	frames: usize,
	started: Option<Instant>,
	expected_parts: u64,
	expected_triangles: u64,
	pipeline_ids: Option<Vec<u64>>,
}

fn pipeline_ids(renderer: &WindowRenderer) -> Option<Vec<u64>> {
	renderer.resource_metrics().map(|metrics| {
		let mut ids = metrics
			.resources
			.iter()
			.filter(|resource| resource.kind == "pipeline")
			.map(|resource| resource.id)
			.collect::<Vec<_>>();
		ids.sort_unstable();
		ids
	})
}

fn check_upload_batch(renderer: &WindowRenderer) {
	use renderer::{MeshUpload, Rgba8Upload, vulkan::Vertex};
	let vertices = [Vertex::new([0.0; 3], [0.0; 2]); 3];
	let indices = [0, 1, 2];
	let mesh = MeshUpload {
		vertices: &vertices,
		indices: &indices,
	};
	let baseline = renderer.resource_metrics();
	let empty = renderer.upload_batch(&[], &[]).unwrap();
	assert!(empty.meshes.is_empty() && empty.textures.is_empty());
	assert!(
		renderer
			.upload_batch(
				&[mesh],
				&[Rgba8Upload {
					width: 1,
					height: 1,
					pixels: &[255; 3]
				}]
			)
			.is_err()
	);
	if let (Some(before), Some(after)) = (&baseline, renderer.resource_metrics()) {
		assert_eq!(
			before.totals.created_resources, after.totals.created_resources,
			"batch validation must precede GPU allocation"
		);
	}
	let batch = renderer
		.upload_batch(
			&[
				mesh,
				MeshUpload {
					vertices: &vertices,
					indices: &[0, 1, 2, 0, 2, 1],
				},
			],
			&[
				Rgba8Upload {
					width: 1,
					height: 1,
					pixels: &[255; 4],
				},
				Rgba8Upload {
					width: 2,
					height: 1,
					pixels: &[128; 8],
				},
			],
		)
		.unwrap();
	assert_eq!(
		batch
			.meshes
			.iter()
			.map(|mesh| mesh.index_count())
			.collect::<Vec<_>>(),
		[3, 6]
	);
	assert_eq!(batch.textures.len(), 2);
	// Ready-on-return permits immediate release without an additional idle wait.
	drop(batch);
	let mut dynamic = renderer.create_dynamic_mesh(3, 3).unwrap();
	renderer
		.write_dynamic_mesh(&mut dynamic, &vertices, &indices)
		.unwrap();
	assert_eq!(dynamic.index_count(), 3);
	assert!(
		renderer
			.write_dynamic_mesh(&mut dynamic, &[vertices[0]; 4], &indices)
			.is_err()
	);
	assert_eq!(dynamic.index_count(), 3);
	renderer.write_dynamic_mesh(&mut dynamic, &[], &[]).unwrap();
	assert_eq!(dynamic.index_count(), 0);
	drop(dynamic);
	if let (Some(before), Some(after)) = (baseline, renderer.resource_metrics()) {
		assert_eq!(before.totals.live_resources, after.totals.live_resources);
		assert_eq!(
			before.totals.allocation_bytes,
			after.totals.allocation_bytes
		);
	}
}

impl ApplicationHandler for Smoke {
	fn resumed(&mut self, events: &ActiveEventLoop) {
		let window = Arc::new(
			events
				.create_window(
					Window::default_attributes()
						.with_title("Renderer lifecycle smoke test")
						.with_visible(false)
						.with_inner_size(winit::dpi::PhysicalSize::new(320, 240)),
				)
				.unwrap(),
		);
		let size = window.inner_size();
		let mut renderer =
			WindowRenderer::new(Arc::clone(&window), size.width, size.height).unwrap();
		renderer.set_lights(&[]).unwrap();
		renderer
			.set_lights(&[
				Light::Directional {
					direction: [0.4, -0.8, -0.6],
					color: [1.0; 3],
					intensity: 0.75,
				},
				Light::Point {
					position: [1.0, 2.0, 3.0],
					color: [1.0, 0.5, 0.2],
					intensity: 2.0,
					range: 10.0,
				},
			])
			.unwrap();
		check_upload_batch(&renderer);
		self.pipeline_ids = pipeline_ids(&renderer);
		let mut assets = assets::AssetManager::new(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"),
		)
		.unwrap();
		let model = Arc::new(assets.load_model(&renderer, "props/robot.glb").unwrap());
		if let Some(metrics) = renderer.resource_metrics() {
			assert!(!metrics.inventory_truncated);
			assert!(metrics.totals.allocation_bytes >= assets.gpu_logical_bytes() as u64);
			assert_eq!(
				metrics
					.resources
					.iter()
					.map(|r| r.allocation_bytes)
					.sum::<u64>(),
				metrics.totals.allocation_bytes
			);
			for kind in [
				"buffer",
				"depth_image",
				"texture_image",
				"pipeline",
				"swapchain_image",
			] {
				assert!(
					metrics
						.totals
						.by_kind
						.get(kind)
						.is_some_and(|count| *count > 0),
					"missing {kind}"
				);
			}
			assert!(
				metrics
					.resources
					.iter()
					.filter(|r| r.kind == "swapchain_image")
					.all(|r| r.allocation_bytes == 0 && r.memory_type.is_none())
			);
		}
		assert!(!model.parts().is_empty());
		let resources = assets.gpu_resource_count();
		let bytes = assets.gpu_logical_bytes();
		assert!(resources > 0 && bytes > 0);
		renderer.wait_idle().unwrap();
		assets.prune_unused();
		assert_eq!(
			(assets.gpu_resource_count(), assets.gpu_logical_bytes()),
			(resources, bytes)
		);
		assets.clear();
		assert_eq!(
			(assets.gpu_resource_count(), assets.gpu_logical_bytes()),
			(resources, bytes),
			"externally retained model allocations must remain charged after cache clear"
		);
		{
			let mut oversized = Scene::default();
			// Reuse GPU Arcs: test the render-object cap without creating large GPU allocations.
			let parts =
				vec![model.parts()[0].clone(); runtime::scene::MAX_SCENE_RENDER_OBJECTS + 1];
			oversized.spawn(ModelInstance::new(
				Arc::new(assets::Model::new(parts)),
				Transform::default(),
			));
			assert!(
				oversized
					.try_render_objects()
					.err()
					.unwrap()
					.contains("65536")
			);
			let frustum =
				runtime::visibility::Frustum::from_view_projection(glam::Mat4::IDENTITY).unwrap();
			assert!(oversized.try_render_visible(Some(&frustum)).is_err());
		}
		self.expected_parts = model.parts().len() as u64;
		self.expected_triangles = model
			.parts()
			.iter()
			.map(|p| p.mesh().index_count() as u64 / 3)
			.sum::<u64>()
			* 257;
		assert!(
			model
				.parts()
				.iter()
				.all(|p| p.material().base_color_factor()[3] >= 1.0)
		);
		let first_transform = Transform {
			position: [-0.8, 0.0, 0.0],
			rotation: [0.2, 0.3, 0.4],
			scale: [0.7; 3],
		};
		let second_transform = Transform {
			position: [0.8, 0.0, 0.0],
			..Transform::default()
		};
		let first = self
			.scene
			.spawn(ModelInstance::new(Arc::clone(&model), first_transform));
		let second = self
			.scene
			.spawn(ModelInstance::new(Arc::clone(&model), second_transform));
		{
			let objects = self.scene.render_objects();
			assert_eq!(objects.len(), model.parts().len() * 2);
			for (index, object) in objects.iter().enumerate() {
				let part = &model.parts()[index % model.parts().len()];
				assert!(std::ptr::eq(object.mesh(), part.mesh()));
				assert!(std::ptr::eq(object.material(), part.material()));
				let transform = if index < model.parts().len() {
					self.scene.instances().next().unwrap().transform()
				} else {
					self.scene.instances().nth(1).unwrap().transform()
				};
				let expected = ModelPushConstants::from_3d(
					transform.position,
					transform.rotation,
					transform.scale,
				);
				assert_eq!(object.transform().model, expected.model);
			}
		}
		drop(self.scene.remove(first).unwrap());
		self.scene.get_mut(second).unwrap().transform_mut().position = [0.0; 3];
		for object in self.scene.render_objects() {
			assert_eq!(
				object.transform().model,
				ModelPushConstants::identity().model
			);
		}
		for index in 0..256 {
			self.scene.spawn(ModelInstance::new(
				Arc::clone(&model),
				Transform {
					position: [
						(index % 16) as f32 * 0.1 - 0.8,
						(index / 16) as f32 * 0.1 - 0.8,
						0.0,
					],
					scale: [0.05; 3],
					..Transform::default()
				},
			));
		}
		self.assets = Some(assets);
		self.renderer = Some(renderer);
		self.window = Some(window);
		self.started = Some(Instant::now());
	}

	fn window_event(&mut self, _events: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
		if let WindowEvent::Resized(size) = event
			&& let Some(renderer) = self.renderer.as_mut()
		{
			renderer.resize(size.width, size.height);
		}
	}

	fn about_to_wait(&mut self, events: &ActiveEventLoop) {
		if self.frames >= 5 {
			return;
		}
		assert!(
			self.started.unwrap().elapsed() < Duration::from_secs(30),
			"smoke test timed out"
		);
		let renderer = self.renderer.as_mut().unwrap();
		let [width, height] = renderer.prepare().unwrap().unwrap();
		let camera = CameraUniform::perspective(
			[0.0, 0.0, 3.0],
			[0.0; 3],
			[0.0, 1.0, 0.0],
			60.0_f32.to_radians(),
			width as f32 / height as f32,
			0.1,
			100.0,
		);
		let objects = self
			.scene
			.try_render_objects()
			.unwrap()
			.into_iter()
			.enumerate()
			.map(|(index, object)| {
				object.with_tint(if index % 2 == 0 {
					[1.0, 0.5, 0.5, 1.0]
				} else {
					[0.5, 1.0, 0.5, 1.0]
				})
			})
			.collect::<Vec<_>>();
		if renderer.draw(&camera, &objects).unwrap() != DrawStatus::Rendered {
			return;
		}
		drop(objects);
		self.frames += 1;
		let metrics = renderer.last_metrics();
		if self.frames <= 2 {
			assert_eq!(metrics.instances, self.expected_parts * 257);
			assert_eq!(metrics.draw_calls, self.expected_parts);
			assert_eq!(metrics.instanced_draw_calls, self.expected_parts);
			assert_eq!(metrics.triangles, self.expected_triangles);
			if self.frames == 1 {
				assert!(
					metrics.gpu_time_ms.is_none(),
					"first frame cannot have completed timestamp results"
				);
			}
			if self.frames == 2 && metrics.gpu_timestamps_supported {
				let ms = metrics
					.gpu_time_ms
					.expect("previous submitted frame timestamp is ready after its fence");
				assert!(ms.is_finite() && ms >= 0.0);
				println!(
					"PASS: instanced metrics={} draws, {} instances, {} triangles; asynchronous GPU timestamp {ms:.3} ms",
					metrics.draw_calls, metrics.instances, metrics.triangles
				);
			}
		} else {
			assert_eq!(
				(metrics.instances, metrics.draw_calls, metrics.triangles),
				(0, 0, 0)
			);
		}
		if self.frames == 2 {
			renderer.wait_idle().unwrap();
			self.scene = Scene::default();
			self.assets.as_mut().unwrap().clear();
			assert_eq!(self.assets.as_ref().unwrap().gpu_resource_count(), 0);
			assert_eq!(self.assets.as_ref().unwrap().gpu_logical_bytes(), 0);
			renderer.resize(0, 0);
			assert_eq!(renderer.prepare().unwrap(), None);
			assert_eq!(renderer.draw(&camera, &[]).unwrap(), DrawStatus::Suspended);
			let _ = self
				.window
				.as_ref()
				.unwrap()
				.request_inner_size(winit::dpi::PhysicalSize::new(640, 360));
			let size = self.window.as_ref().unwrap().inner_size();
			renderer.resize(size.width, size.height);
		}
		if self.frames == 5 {
			assert_eq!([width, height], [640, 360], "resized surface extent");
			assert_eq!(
				pipeline_ids(renderer),
				self.pipeline_ids,
				"extent-only resize must retain graphics pipelines"
			);
			renderer.wait_idle().unwrap();
			self.assets = None;
			self.renderer = None;
			self.window = None;
			events.exit();
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
	let mut config = engine_logger::Config::new("renderer-smoke");
	config.error_backtraces = false;
	config.console = false;
	if let Some(directory) = std::env::var_os("ENGINE_LOG_DIR") {
		config.storage = Some(engine_logger::StorageConfig::new(directory));
	}
	let _logging = engine_logger::init(config).unwrap();
	let events = EventLoop::new().unwrap();
	events.set_control_flow(ControlFlow::Poll);
	let mut smoke = Smoke::default();
	events.run_app(&mut smoke).unwrap();
	assert_eq!(smoke.frames, 5);
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
		"PASS: scene resource mapping, render-object budget, retained GPU accounting/pruning, instanced tinted/lit draws, resource release, zero-size suspension, empty resize/draw, shutdown"
	);
}
