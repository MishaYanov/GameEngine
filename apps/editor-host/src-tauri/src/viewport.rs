//! Native Vulkan child viewport. Angular decides every tool/input binding.
//! GPU ownership stays on one worker; idle Edit mode waits for invalidation.
use std::sync::{Arc, Mutex};

use bridge::editor::{ViewportBoundsDto, ViewportStatusDto};

use crate::editor::EditorWorkspace;

type Workspace = Arc<Mutex<Option<EditorWorkspace>>>;

pub struct ViewportHost {
	shared: Workspace,
	status: Arc<Mutex<ViewportStatusDto>>,
	shutting_down: bool,
	#[cfg(windows)]
	worker_started: bool,
	#[cfg(windows)]
	running: Mutex<Option<windows::Running>>,
}

impl ViewportHost {
	pub fn new(shared: Workspace) -> Self {
		Self {
			shared,
			status: Arc::new(Mutex::new(ViewportStatusDto::default())),
			shutting_down: false,
			#[cfg(windows)]
			worker_started: false,
			#[cfg(windows)]
			running: Mutex::new(None),
		}
	}

	pub fn attach(
		&mut self,
		parent: &tauri::WebviewWindow,
		bounds: ViewportBoundsDto,
	) -> Result<ViewportStatusDto, String> {
		if self.shutting_down {
			return Err("Viewport host is shutting down".into());
		}
		bounds.validate()?;
		#[cfg(windows)]
		{
			self.reap();
			let mut running = self
				.running
				.lock()
				.map_err(|_| "Viewport lifecycle lock is poisoned")?;
			if let Some(worker) = running.as_ref() {
				if worker.control.stopping() {
					return Err(
						"Previous viewport is still shutting down; retry after it finishes".into(),
					);
				}
				if worker.parent.hwnd().map_err(|error| error.to_string())?
					!= parent.hwnd().map_err(|error| error.to_string())?
				{
					return Err("An embedded viewport cannot move between editor windows".into());
				}
				worker.control.attach(
					bounds,
					parent.scale_factor().map_err(|error| error.to_string())?,
				)?;
				drop(running);
				return Ok(self.status());
			}
			if self.worker_started {
				return Err("The native viewport event loop has stopped; restart the editor to attach again".into());
			}
			let hwnd = parent.hwnd().map_err(|error| error.to_string())?.0 as isize;
			if hwnd == 0 {
				return Err("Editor window has no native handle".into());
			}
			let dpi = parent.scale_factor().map_err(|error| error.to_string())?;
			let control = Arc::new(windows::Control::new(bounds, dpi)?);
			*self
				.status
				.lock()
				.map_err(|_| "Viewport status lock is poisoned")? = ViewportStatusDto {
				attached: true,
				..ViewportStatusDto::default()
			};
			let worker = windows::spawn(
				hwnd,
				parent.clone(),
				Arc::clone(&self.shared),
				Arc::clone(&control),
				Arc::clone(&self.status),
			)?;
			self.worker_started = true;
			*running = Some(windows::Running {
				control,
				worker,
				parent: parent.clone(),
			});
			drop(running);
			Ok(self.status())
		}
		#[cfg(not(windows))]
		{
			let _ = parent;
			Err("The embedded Vulkan viewport currently supports Windows only".into())
		}
	}

	pub fn bounds(&mut self, bounds: ViewportBoundsDto) -> Result<ViewportStatusDto, String> {
		if self.shutting_down {
			return Err("Viewport host is shutting down".into());
		}
		bounds.validate()?;
		#[cfg(windows)]
		{
			self.reap();
			{
				let running = self
					.running
					.lock()
					.map_err(|_| "Viewport lifecycle lock is poisoned")?;
				let running = running.as_ref().ok_or("No viewport is attached")?;
				let dpi = running
					.parent
					.scale_factor()
					.map_err(|error| error.to_string())?;
				running.control.set_bounds(bounds, dpi)?;
			}
			Ok(self.status())
		}
		#[cfg(not(windows))]
		Err("The embedded Vulkan viewport currently supports Windows only".into())
	}

	pub fn invalidate(&self) {
		#[cfg(windows)]
		if let Ok(running) = self.running.lock() {
			if let Some(running) = running.as_ref() {
				running.control.invalidate();
			}
		}
	}

	pub fn detach(&mut self) {
		#[cfg(windows)]
		if let Ok(running) = self.running.lock() {
			if let Some(running) = running.as_ref() {
				running.control.detach();
			}
		}
		if let Ok(mut status) = self.status.lock() {
			status.attached = false;
			status.visible = false;
			status.draw_calls = 0;
			status.instances = 0;
			status.triangles = 0;
			status.gpu_time_ms = None;
			status.width = 0;
			status.height = 0;
		}
		self.reap();
	}

	pub fn set_parent_visible(&self, visible: bool) {
		#[cfg(windows)]
		if let Ok(running) = self.running.lock() {
			if let Some(running) = running.as_ref() {
				running.control.set_parent_visible(visible);
			}
		}
		#[cfg(not(windows))]
		let _ = visible;
	}

	/// CloseRequested must first be prevented. The real parent HWND is destroyed
	/// only after the child worker has waited for GPU completion and exited.
	pub fn close_parent_when_ready(&mut self, parent: tauri::WebviewWindow) {
		if self.shutting_down {
			return;
		}
		self.shutting_down = true;
		#[cfg(windows)]
		{
			let running = self
				.running
				.get_mut()
				.unwrap_or_else(std::sync::PoisonError::into_inner)
				.take();
			if let Some(running) = running {
				running.control.stop();
				std::thread::spawn(move || {
					running.join();
					let _ = parent.destroy();
				});
				return;
			}
		}
		let _ = parent.destroy();
	}

	pub fn status(&self) -> ViewportStatusDto {
		self.reap();
		let status = self
			.status
			.lock()
			.map(|status| status.clone())
			.unwrap_or_else(|_| ViewportStatusDto {
				error: Some("Viewport status lock is poisoned".into()),
				..ViewportStatusDto::default()
			});
		#[cfg(windows)]
		if let Ok(running) = self.running.lock() {
			if let Some(running) = running.as_ref() {
				return running.control.requested_status(status);
			}
		}
		status
	}

	fn reap(&self) {
		#[cfg(windows)]
		if let Ok(mut running) = self.running.lock() {
			if running
				.as_ref()
				.is_some_and(|running| running.worker.is_finished())
			{
				if let Some(running) = running.take() {
					running.join();
				}
			}
		}
	}
}

impl Drop for ViewportHost {
	fn drop(&mut self) {
		#[cfg(windows)]
		{
			let running = self
				.running
				.get_mut()
				.unwrap_or_else(std::sync::PoisonError::into_inner);
			if let Some(running) = running.take() {
				running.control.stop();
				// Teardown may wait for a GPU fence. Keep the parent alive until
				// that completes without blocking Tauri's UI/event-loop thread.
				std::thread::spawn(move || running.join());
			}
		}
	}
}

#[cfg(windows)]
mod windows {
	use super::*;
	use bridge::editor::{ViewportInputDto, ViewportModifiersDto};
	use raw_window_handle::{RawWindowHandle, Win32WindowHandle};
	use renderer::{vulkan::CameraUniform, DrawStatus, WindowRenderer};
	use runtime::{assets::AssetManager, scene::Scene, scene_io, Camera};
	use std::{
		num::NonZeroIsize,
		path::PathBuf,
		thread::JoinHandle,
		time::{Duration, Instant},
	};
	use tauri::Emitter;
	use winit::{
		application::ApplicationHandler,
		dpi::{PhysicalPosition, PhysicalSize},
		event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
		event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
		keyboard::{ModifiersState, PhysicalKey},
		platform::windows::EventLoopBuilderExtWindows,
		window::{Window, WindowId},
	};

	const FRAME: Duration = Duration::from_nanos(16_666_667);

	#[derive(Clone, Copy)]
	struct Wake;

	struct Latest {
		bounds: ViewportBoundsDto,
		dpi: f64,
		invalidated: bool,
		attached: bool,
		parent_visible: bool,
		clear_input: bool,
		stop: bool,
		wake_queued: bool,
	}

	pub(super) struct Control {
		latest: Mutex<Latest>,
		proxy: Mutex<Option<EventLoopProxy<Wake>>>,
	}
	impl Control {
		pub(super) fn requested_status(&self, mut status: ViewportStatusDto) -> ViewportStatusDto {
			if let Ok(latest) = self.latest.lock() {
				status.attached = latest.attached && !latest.stop;
				status.visible &= status.attached
					&& latest.parent_visible
					&& latest.bounds.visible
					&& latest.bounds.width > 0.0
					&& latest.bounds.height > 0.0;
				if !status.attached {
					status.width = 0;
					status.height = 0;
					status.draw_calls = 0;
					status.instances = 0;
					status.triangles = 0;
					status.gpu_time_ms = None;
				}
			}
			status
		}
		pub(super) fn new(bounds: ViewportBoundsDto, dpi: f64) -> Result<Self, String> {
			Self::validate_bounds(bounds, dpi)?;
			Ok(Self {
				latest: Mutex::new(Latest {
					bounds,
					dpi,
					invalidated: true,
					attached: true,
					parent_visible: true,
					clear_input: true,
					stop: false,
					wake_queued: false,
				}),
				proxy: Mutex::new(None),
			})
		}
		fn validate_bounds(bounds: ViewportBoundsDto, dpi: f64) -> Result<(), String> {
			bounds.validate()?;
			if !dpi.is_finite() || dpi <= 0.0 || dpi > 16.0 {
				return Err("Invalid editor display scale factor".into());
			}
			let width = bounds.width * dpi;
			let height = bounds.height * dpi;
			if width > 8192.0 || height > 8192.0 || width * height > 16_777_216.0 {
				return Err("Viewport physical extent exceeds the 8192-pixel axis / 16-megapixel allocation limit".into());
			}
			Ok(())
		}
		pub(super) fn attach(&self, bounds: ViewportBoundsDto, dpi: f64) -> Result<(), String> {
			self.set_bounds(bounds, dpi)?;
			if let Ok(mut latest) = self.latest.lock() {
				latest.attached = true;
				latest.invalidated = true;
			}
			self.wake();
			Ok(())
		}
		pub(super) fn detach(&self) {
			if let Ok(mut latest) = self.latest.lock() {
				latest.attached = false;
				latest.invalidated = true;
				latest.clear_input = true;
			}
			self.wake();
		}
		pub(super) fn set_parent_visible(&self, visible: bool) {
			if let Ok(mut latest) = self.latest.lock() {
				latest.parent_visible = visible;
				latest.invalidated = true;
				latest.clear_input |= !visible;
			}
			self.wake();
		}
		fn wake(&self) {
			let send = self
				.latest
				.lock()
				.map(|mut latest| {
					if latest.wake_queued {
						false
					} else {
						latest.wake_queued = true;
						true
					}
				})
				.unwrap_or(false);
			if send {
				if let Ok(proxy) = self.proxy.lock() {
					if let Some(proxy) = proxy.as_ref() {
						let _ = proxy.send_event(Wake);
					}
				}
			}
		}
		pub(super) fn set_bounds(&self, bounds: ViewportBoundsDto, dpi: f64) -> Result<(), String> {
			Self::validate_bounds(bounds, dpi)?;
			let mut latest = self
				.latest
				.lock()
				.map_err(|_| "Viewport control lock is poisoned")?;
			if latest.stop {
				return Err("Viewport is shutting down".into());
			}
			latest.bounds = bounds;
			latest.dpi = dpi;
			latest.invalidated = true;
			latest.clear_input |= !bounds.visible || bounds.width == 0.0 || bounds.height == 0.0;
			drop(latest);
			self.wake();
			Ok(())
		}
		pub(super) fn invalidate(&self) {
			if let Ok(mut latest) = self.latest.lock() {
				latest.invalidated = true;
			}
			self.wake();
		}
		pub(super) fn stop(&self) {
			if let Ok(mut latest) = self.latest.lock() {
				latest.stop = true;
			}
			self.wake();
		}
		pub(super) fn stopping(&self) -> bool {
			self.latest.lock().map(|latest| latest.stop).unwrap_or(true)
		}
	}

	pub(super) struct Running {
		pub(super) control: Arc<Control>,
		pub(super) worker: JoinHandle<()>,
		pub(super) parent: tauri::WebviewWindow,
	}
	impl Running {
		pub(super) fn join(self) {
			let Self { worker, parent, .. } = self;
			let _ = worker.join();
			drop(parent);
		}
	}

	pub(super) fn spawn(
		hwnd: isize,
		parent: tauri::WebviewWindow,
		shared: Workspace,
		control: Arc<Control>,
		status: Arc<Mutex<ViewportStatusDto>>,
	) -> Result<JoinHandle<()>, String> {
		std::thread::Builder::new()
			.name("editor-vulkan-viewport".into())
			.spawn(move || {
				let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
					let mut builder = EventLoop::<Wake>::with_user_event();
					builder.with_any_thread(true);
					let event_loop = builder.build().map_err(|error| error.to_string())?;
					*control
						.proxy
						.lock()
						.map_err(|_| "Viewport proxy lock is poisoned")? =
						Some(event_loop.create_proxy());
					let mut app = Worker::new(
						hwnd,
						parent,
						shared,
						Arc::clone(&control),
						Arc::clone(&status),
					);
					event_loop
						.run_app(&mut app)
						.map_err(|error| error.to_string())
				}));
				let result = match result {
					Ok(result) => {
						if result.is_err() {
							engine_logger::error!(target: "editor_viewport", "Viewport worker failed: operation=event_loop");
						}
						result
					}
					// The process panic hook already owns the ERROR record.
					Err(_) => Err("Embedded viewport worker panicked".into()),
				};
				if let Ok(mut current) = status.lock() {
					current.attached = false;
					current.visible = false;
					if let Err(error) = result {
						current.error = Some(error);
					}
				}
				if let Ok(mut proxy) = control.proxy.lock() {
					*proxy = None;
				}
			})
			.map_err(|error| {
				engine_logger::error!(target: "editor_viewport", "Viewport worker failed: operation=thread_start");
				format!("Failed to start embedded viewport worker: {error}")
			})
	}

	// Order matters: uploaded models/assets retire before the renderer/device.
	struct GpuState {
		scene: Scene,
		assets: AssetManager,
		renderer: WindowRenderer,
		asset_root: PathBuf,
		camera: Camera,
		revision: u64,
		generation: u64,
		change_cursor: u64,
	}

	struct RenderPacket {
		document: Option<Arc<runtime::SceneData>>,
		camera: Camera,
		revision: u64,
		generation: u64,
		change_cursor: u64,
		asset_root: PathBuf,
		lights_changed: bool,
	}
	impl Drop for GpuState {
		fn drop(&mut self) {
			if self.renderer.wait_idle().is_err() {
				engine_logger::error!(target: "editor_viewport", "Viewport teardown failed: operation=gpu_wait_idle, generation={}", self.generation);
			}
			engine_logger::debug!(target: "editor_viewport", "Viewport GPU state retired: generation={}", self.generation);
		}
	}

	struct Worker {
		gpu: Option<GpuState>,
		window: Option<Arc<Window>>,
		hwnd: isize,
		parent: tauri::WebviewWindow,
		shared: Workspace,
		control: Arc<Control>,
		status: Arc<Mutex<ViewportStatusDto>>,
		bounds: ViewportBoundsDto,
		attached: bool,
		parent_visible: bool,
		dpi: f64,
		dirty: bool,
		occluded: bool,
		failed: bool,
		last_tick: Option<Instant>,
		next_frame: Instant,
		redraw_retry: Option<Instant>,
		prepared_extent: Option<[u32; 2]>,
		cursor: [f64; 2],
		modifiers: ModifiersState,
		pending_motion: Option<[f64; 2]>,
		last_motion: Instant,
		last_status: Instant,
	}

	impl Worker {
		fn new(
			hwnd: isize,
			parent: tauri::WebviewWindow,
			shared: Workspace,
			control: Arc<Control>,
			status: Arc<Mutex<ViewportStatusDto>>,
		) -> Self {
			let now = Instant::now();
			Self {
				gpu: None,
				window: None,
				hwnd,
				parent,
				shared,
				control,
				status,
				bounds: ViewportBoundsDto::default(),
				attached: false,
				parent_visible: true,
				dpi: 1.0,
				dirty: true,
				occluded: false,
				failed: false,
				last_tick: None,
				next_frame: now,
				redraw_retry: None,
				prepared_extent: None,
				cursor: [0.0; 2],
				modifiers: ModifiersState::empty(),
				pending_motion: None,
				last_motion: now - FRAME,
				last_status: now - Duration::from_secs(1),
			}
		}
		fn fail(&mut self, operation: &'static str, error: impl Into<String>) {
			if !self.failed {
				engine_logger::error!(target: "editor_viewport", "Viewport entered failed state: operation={operation}");
			}
			self.failed = true;
			self.dirty = false;
			self.last_tick = None;
			self.prepared_extent = None;
			if let Ok(mut status) = self.status.lock() {
				status.error = Some(error.into());
				status.width = 0;
				status.height = 0;
			}
		}
		fn active(&self) -> bool {
			self.attached
				&& self.parent_visible
				&& self.bounds.visible
				&& self.bounds.width > 0.0
				&& self.bounds.height > 0.0
				&& !self.occluded
		}
		fn clear_play_input(&mut self) {
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					workspace.backend.clear_play_input();
				}
			}
			self.last_tick = None;
		}
		fn extent(&self) -> [u32; 2] {
			[
				(self.bounds.width * self.dpi).round() as u32,
				(self.bounds.height * self.dpi).round() as u32,
			]
		}
		fn apply_control(&mut self, event_loop: &ActiveEventLoop) {
			let was_active = self.active();
			let values = self
				.control
				.latest
				.lock()
				.map(|mut latest| {
					let values = (
						latest.bounds,
						latest.dpi,
						latest.invalidated,
						latest.attached,
						latest.parent_visible,
						latest.clear_input,
						latest.stop,
					);
					latest.invalidated = false;
					latest.wake_queued = false;
					latest.clear_input = false;
					values
				})
				.map_err(|_| ());
			let Ok((bounds, dpi, invalidated, attached, parent_visible, clear_input, stop)) =
				values
			else {
				self.fail("control_state", "Viewport control lock is poisoned");
				event_loop.exit();
				return;
			};
			if stop {
				event_loop.exit();
				return;
			}
			let changed = self.attached != attached
				|| self.parent_visible != parent_visible
				|| self.bounds.x != bounds.x
				|| self.bounds.y != bounds.y
				|| self.bounds.width != bounds.width
				|| self.bounds.height != bounds.height
				|| self.bounds.visible != bounds.visible
				|| self.dpi != dpi;
			if self.attached != attached || (self.failed && invalidated) {
				self.failed = false;
				if let Ok(mut status) = self.status.lock() {
					status.error = None;
				}
			}
			self.attached = attached;
			self.parent_visible = parent_visible;
			self.bounds = bounds;
			self.dpi = dpi;
			if clear_input || !self.active() {
				self.clear_play_input();
			}
			if changed {
				if let Some(window) = &self.window {
					let [width, height] = self.extent();
					window.set_outer_position(PhysicalPosition::new(
						(bounds.x * dpi).round() as i32,
						(bounds.y * dpi).round() as i32,
					));
					let _ =
						window.request_inner_size(PhysicalSize::new(width.max(1), height.max(1)));
					window.set_visible(self.active());
					if let Some(gpu) = self.gpu.as_mut() {
						gpu.renderer.resize(width, height);
					}
				}
			}
			if invalidated || changed {
				self.dirty = true;
			}
			if !self.active() {
				self.last_tick = None;
				self.pending_motion = None;
				if was_active {
					self.modifiers = ModifiersState::empty();
					self.emit_input("blurred", None, None, false, None);
				}
			}
			if !self.attached {
				self.gpu = None;
				self.prepared_extent = None;
			}
			let extent = self.prepared_extent.unwrap_or([0; 2]);
			if let Ok(mut status) = self.status.lock() {
				status.attached = self.attached;
				status.visible = self.active();
				status.width = extent[0];
				status.height = extent[1];
			}
			if changed {
				self.last_status = Instant::now() - Duration::from_secs(1);
			}
		}

		fn draw(&mut self) -> Result<(), String> {
			if !self.active() {
				return Ok(());
			}
			let packet = loop {
				let captured = {
					let workspace = self
						.shared
						.lock()
						.map_err(|_| "Editor document lock is poisoned")?;
					let workspace = workspace
						.as_ref()
						.ok_or("Open a scene before attaching a viewport")?;
					if self
						.gpu
						.as_ref()
						.is_some_and(|gpu| gpu.asset_root != workspace.asset_root)
					{
						None
					} else {
						let session = workspace.backend.session();
						let revision = session.scene_revision();
						let generation = workspace.generation;
						let cursor = workspace.backend.change_cursor();
						let mut synchronized = false;
						let mut lights_changed = false;
						// Only CPU metadata/matrices are touched under the workspace
						// lock. Descriptor uploads, waits, acquisition and presentation
						// happen after this packet has released the lock.
						if let Some(gpu) = self.gpu.as_mut() {
							if gpu.asset_root == workspace.asset_root
								&& gpu.generation == generation
							{
								if gpu.revision == revision {
									synchronized = true;
								} else {
									let page = workspace
										.backend
										.changes_since(gpu.change_cursor)
										.map_err(|error| error.to_string())?;
									let mut changes = runtime::world::WorldChange {
										full_resync: page.resync_required,
										..runtime::world::WorldChange::default()
									};
									for event in page.events {
										match event.event {
											runtime::events::BackendEventKind::DocumentChanged {
												changes: delta,
											}
											| runtime::events::BackendEventKind::SimulationAdvanced {
												changes: delta,
												..
											} => changes.merge(delta),
											runtime::events::BackendEventKind::StateChanged
											| runtime::events::BackendEventKind::ProjectChanged => {}
										}
									}
									if changes.is_empty() {
										changes.full_resync = true;
									}
									synchronized = if let Some(world) = session.render_world() {
										gpu.scene.synchronize_world(world, &changes)?
									} else {
										gpu.scene.synchronize_authored(
											session.validated_authored_document(),
											&changes,
										)?
									};
									lights_changed = changes.lights_changed || changes.full_resync;
								}
								if synchronized {
									gpu.revision = revision;
									gpu.change_cursor = cursor;
								}
							}
						}
						let document = if synchronized {
							None
						} else {
							Some(session.render_document())
						};
						let camera = if let Some(world) = session.render_world() {
							world.data().camera.clone()
						} else if let Some(document) = document.as_ref() {
							document.camera.clone()
						} else {
							session.authored_document().camera.clone()
						};
						Some(RenderPacket {
							document,
							camera: Camera {
								position: camera.position,
								yaw: camera.yaw,
								pitch: camera.pitch,
								vertical_fov: camera.vertical_fov,
								near: camera.near_plane,
								far: camera.far_plane,
							},
							revision,
							generation,
							change_cursor: cursor,
							asset_root: workspace.asset_root.clone(),
							lights_changed,
						})
					}
				};
				if let Some(packet) = captured {
					break packet;
				}
				// Source-root retirement can wait on the GPU. No workspace
				// lock or strong document snapshot survives into that wait.
				self.gpu = None;
				self.prepared_extent = None;
			};
			let RenderPacket {
				mut document,
				camera,
				revision,
				generation,
				change_cursor,
				asset_root,
				lights_changed,
			} = packet;
			if self.gpu.is_none() {
				let window = Arc::clone(
					self.window
						.as_ref()
						.ok_or("Native viewport window is not initialized")?,
				);
				let [width, height] = self.extent();
				let mut renderer = WindowRenderer::new(Arc::clone(&window), width, height)?;
				let mut assets =
					AssetManager::new(&asset_root).map_err(|error| error.to_string())?;
				let data = document
					.take()
					.ok_or("Initial viewport packet is missing scene data")?;
				let (scene, initial_camera) = scene_io::instantiate(&data, &mut assets, &renderer)?;
				renderer.set_lights(&scene.renderer_lights())?;
				engine_logger::debug!(target: "editor_viewport", "Viewport GPU state created: generation={generation}, width={width}, height={height}, entities={}, lights={}", data.entities.len(), data.lights.len());
				drop(data);
				self.gpu = Some(GpuState {
					scene,
					assets,
					renderer,
					asset_root,
					camera: initial_camera,
					revision,
					generation,
					change_cursor,
				});
			}
			let gpu = self
				.gpu
				.as_mut()
				.ok_or("Viewport GPU state is not initialized")?;
			let generation_changed = gpu.generation != generation;
			if gpu.revision != revision || generation_changed {
				let data = document
					.take()
					.ok_or("Full viewport synchronization packet is missing scene data")?;
				let replacement = if !gpu.scene.synchronize(&data)? {
					let (scene, _) =
						match scene_io::instantiate(&data, &mut gpu.assets, &gpu.renderer) {
							Ok(candidate) => candidate,
							Err(error) => {
								drop(data);
								gpu.renderer.wait_idle().map_err(|wait_error| {
									format!(
										"{error}; failed to retire rejected candidate GPU resources: {wait_error}"
									)
								})?;
								gpu.assets.prune_unused();
								return Err(error);
							}
						};
					Some(scene)
				} else {
					None
				};
				drop(data);
				if let Some(scene) = replacement {
					gpu.renderer.wait_idle()?;
					gpu.scene = scene;
					gpu.assets.prune_unused();
				} else if generation_changed {
					gpu.renderer.wait_idle()?;
					gpu.assets.prune_unused();
				}
				gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
				gpu.revision = revision;
				gpu.generation = generation;
				gpu.change_cursor = change_cursor;
			} else if lights_changed {
				gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
			}
			gpu.camera = camera;
			// Do not hold a strong document snapshot across frame fences or
			// presentation. Play can mutate its uniquely owned Arc in place.
			drop(document);
			let Some([width, height]) = gpu.renderer.prepare()? else {
				self.prepared_extent = None;
				if let Ok(mut status) = self.status.lock() {
					status.width = 0;
					status.height = 0;
				}
				return Ok(());
			};
			if self.prepared_extent != Some([width, height]) {
				self.last_status = Instant::now() - Duration::from_secs(1);
			}
			self.prepared_extent = Some([width, height]);
			let uniform = CameraUniform::perspective(
				gpu.camera.position,
				gpu.camera.target(),
				[0.0, 1.0, 0.0],
				gpu.camera.vertical_fov,
				width as f32 / height as f32,
				gpu.camera.near,
				gpu.camera.far,
			);
			let frustum = runtime::visibility::Frustum::from_view_projection(
				glam::Mat4::from_cols_array(&uniform.view_projection),
			)?;
			let (objects, _) = gpu.scene.try_render_visible(Some(&frustum))?;
			match gpu.renderer.draw(&uniform, &objects)? {
				DrawStatus::NeedsRedraw => {
					self.dirty = true;
					self.redraw_retry = Some(Instant::now() + FRAME);
				}
				DrawStatus::Rendered | DrawStatus::Suspended => {
					self.redraw_retry = None;
				}
			}
			let metrics = gpu.renderer.last_metrics();
			if let Ok(mut status) = self.status.lock() {
				if status.attached {
					status.width = width;
					status.height = height;
					status.draw_calls = metrics.draw_calls as usize;
					status.instances = metrics.instances as usize;
					status.triangles = metrics.triangles as usize;
					status.gpu_time_ms = metrics.gpu_time_ms;
					status.gpu_timestamps_supported = metrics.gpu_timestamps_supported;
				}
			}
			Ok(())
		}

		fn emit_input(
			&self,
			kind: &str,
			button: Option<String>,
			code: Option<String>,
			repeat: bool,
			delta: Option<[f64; 2]>,
		) {
			let extent = self.prepared_extent.unwrap_or([0; 2]);
			let position = self.window.as_ref().and_then(|window| {
				let size = window.inner_size();
				if size.width == 0 || size.height == 0 || extent.contains(&0) {
					return None;
				}
				Some([
					self.cursor[0] * extent[0] as f64 / size.width as f64,
					self.cursor[1] * extent[1] as f64 / size.height as f64,
				])
			});
			let payload = ViewportInputDto {
				kind: kind.into(),
				position,
				extent,
				button,
				code,
				repeat,
				delta,
				modifiers: ViewportModifiersDto {
					shift: self.modifiers.shift_key(),
					control: self.modifiers.control_key(),
					alt: self.modifiers.alt_key(),
					super_key: self.modifiers.super_key(),
				},
			};
			let _ = self.parent.emit("editor://viewport-input", payload);
		}
	}

	impl ApplicationHandler<Wake> for Worker {
		fn resumed(&mut self, event_loop: &ActiveEventLoop) {
			if self.window.is_some() {
				return;
			}
			self.apply_control(event_loop);
			if self.control.stopping() {
				return;
			}
			let Some(hwnd) = NonZeroIsize::new(self.hwnd) else {
				self.fail("native_parent", "Invalid editor parent HWND");
				event_loop.exit();
				return;
			};
			let [width, height] = self.extent();
			// SAFETY: HWND originates from Tauri's backend; Running and Worker
			// retain parent clones until this event loop and its child exit.
			let attributes = unsafe {
				Window::default_attributes()
					.with_parent_window(Some(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd))))
			}
			.with_title("Game Engine viewport")
			.with_decorations(false)
			.with_visible(false)
			.with_inner_size(PhysicalSize::new(width.max(1), height.max(1)))
			.with_position(PhysicalPosition::new(
				(self.bounds.x * self.dpi).round() as i32,
				(self.bounds.y * self.dpi).round() as i32,
			));
			match event_loop.create_window(attributes) {
				Ok(window) => {
					let window = Arc::new(window);
					window.set_visible(self.active());
					self.window = Some(window);
					self.dirty = true;
				}
				Err(error) => {
					self.fail("window_create", error.to_string());
					event_loop.exit();
				}
			}
		}
		fn user_event(&mut self, event_loop: &ActiveEventLoop, _: Wake) {
			self.apply_control(event_loop);
		}
		fn window_event(
			&mut self,
			event_loop: &ActiveEventLoop,
			window_id: WindowId,
			event: WindowEvent,
		) {
			if self
				.window
				.as_ref()
				.is_none_or(|window| window.id() != window_id)
			{
				return;
			}
			match event {
				WindowEvent::CloseRequested | WindowEvent::Destroyed => {
					event_loop.exit();
				}
				WindowEvent::Resized(size) => {
					if let Some(gpu) = self.gpu.as_mut() {
						gpu.renderer.resize(size.width, size.height);
					}
					self.dirty = true;
				}
				WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
					if let Err(error) = Control::validate_bounds(self.bounds, scale_factor) {
						self.fail("display_scale", error);
						if let Some(window) = self.window.as_ref() {
							window.set_visible(false);
						}
						return;
					}
					if let Ok(mut latest) = self.control.latest.lock() {
						latest.dpi = scale_factor;
						latest.invalidated = true;
					}
					self.apply_control(event_loop);
				}
				WindowEvent::Occluded(occluded) => {
					self.occluded = occluded;
					self.last_tick = None;
					self.dirty = !occluded;
					if occluded {
						self.clear_play_input();
					}
				}
				WindowEvent::RedrawRequested => {
					if !self.failed {
						if self
							.redraw_retry
							.is_some_and(|deadline| Instant::now() < deadline)
						{
							self.dirty = true;
						} else if let Err(error) = self.draw() {
							self.fail("gpu_update", error);
						}
					}
				}
				WindowEvent::CursorMoved { position, .. } => {
					self.cursor = [position.x, position.y];
					self.pending_motion = Some(self.cursor);
				}
				WindowEvent::CursorLeft { .. } => {
					self.pending_motion = None;
					self.emit_input("pointer_leave", None, None, false, None);
				}
				WindowEvent::MouseInput { state, button, .. } => {
					let button = match button {
						MouseButton::Left => "left".into(),
						MouseButton::Right => "right".into(),
						MouseButton::Middle => "middle".into(),
						MouseButton::Back => "back".into(),
						MouseButton::Forward => "forward".into(),
						MouseButton::Other(value) => format!("other:{value}"),
					};
					self.emit_input(
						if state == ElementState::Pressed {
							"pointer_down"
						} else {
							"pointer_up"
						},
						Some(button),
						None,
						false,
						None,
					);
				}
				WindowEvent::MouseWheel { delta, .. } => {
					let (kind, delta) = match delta {
						MouseScrollDelta::LineDelta(x, y) => ("wheel_lines", [x as f64, y as f64]),
						MouseScrollDelta::PixelDelta(position) => {
							("wheel_pixels", [position.x, position.y])
						}
					};
					self.emit_input(kind, None, None, false, Some(delta));
				}
				WindowEvent::ModifiersChanged(modifiers) => {
					self.modifiers = modifiers.state();
				}
				WindowEvent::KeyboardInput { event, .. } => {
					let code = match event.physical_key {
						PhysicalKey::Code(code) => format!("{code:?}"),
						PhysicalKey::Unidentified(code) => format!("{code:?}"),
					};
					self.emit_input(
						if event.state == ElementState::Pressed {
							"key_down"
						} else {
							"key_up"
						},
						None,
						Some(code),
						event.repeat,
						None,
					);
				}
				WindowEvent::Focused(focused) => {
					if !focused {
						self.modifiers = ModifiersState::empty();
						self.pending_motion = None;
						self.clear_play_input();
					}
					self.emit_input(
						if focused { "focused" } else { "blurred" },
						None,
						None,
						false,
						None,
					);
				}
				_ => {}
			}
		}
		fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
			if self.control.stopping() {
				event_loop.exit();
				return;
			}
			let now = Instant::now();
			if self.pending_motion.is_some() && now.duration_since(self.last_motion) >= FRAME {
				self.pending_motion = None;
				self.last_motion = now;
				self.emit_input("pointer_move", None, None, false, None);
			}
			let ticking = self
				.shared
				.lock()
				.ok()
				.and_then(|workspace| {
					workspace
						.as_ref()
						.map(|workspace| workspace.backend.session().needs_tick())
				})
				.unwrap_or(false);
			let ticking = ticking && self.active() && !self.failed;
			if ticking && now >= self.next_frame {
				let elapsed = self
					.last_tick
					.replace(now)
					.map_or(0.0, |last| now.duration_since(last).as_secs_f64());
				let result = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned".to_string())
					.and_then(|mut workspace| {
						let backend = &mut workspace
							.as_mut()
							.ok_or("No editor document is open".to_string())?
							.backend;
						let outcome = backend.tick(elapsed).map_err(|error| error.to_string())?;
						Ok(!outcome.changes.is_empty())
					});
				match result {
					Ok(changed) => self.dirty |= changed,
					Err(error) => self.fail("simulation_tick", error),
				}
				self.next_frame = now + FRAME;
			} else if !ticking {
				self.last_tick = None;
				self.next_frame = now;
			}
			if self.dirty
				&& self.active()
				&& !self.failed
				&& self.redraw_retry.is_none_or(|deadline| now >= deadline)
			{
				self.dirty = false;
				if let Some(window) = self.window.as_ref() {
					window.request_redraw();
				}
			}
			if now.duration_since(self.last_status) >= Duration::from_millis(500) {
				let status = self.status.lock().ok().map(|status| status.clone());
				if let Some(status) = status {
					let _ = self.parent.emit(
						"editor://viewport-status",
						self.control.requested_status(status),
					);
				}
				self.last_status = now;
			}
			let mut deadline = if ticking { Some(self.next_frame) } else { None };
			if self.pending_motion.is_some() {
				let motion = self.last_motion + FRAME;
				deadline = Some(deadline.map_or(motion, |deadline| deadline.min(motion)));
			}
			if self.dirty && self.active() && !self.failed {
				if let Some(retry) = self.redraw_retry {
					deadline = Some(deadline.map_or(retry, |deadline| deadline.min(retry)));
				}
			}
			event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
		}
		fn exiting(&mut self, _: &ActiveEventLoop) {
			self.gpu = None;
			self.window = None;
			engine_logger::debug!(target: "editor_viewport", "Viewport worker teardown completed");
		}
	}
}
