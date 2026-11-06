//! Native Vulkan child viewport and spatial input; Angular supplies adjacent controls.
//! GPU ownership stays on one worker; idle Edit mode waits for invalidation.
use bridge::viewport::{ViewportActionDto, ViewportEditorStateDto};
use std::sync::{mpsc, Arc, Mutex};

use bridge::editor::{ViewportBoundsDto, ViewportStatusDto};

use crate::editor::EditorWorkspace;

type Workspace = Arc<Mutex<Option<EditorWorkspace>>>;

pub struct ViewportHost {
	shared: Workspace,
	status: Arc<Mutex<ViewportStatusDto>>,
	editor_state: Arc<Mutex<ViewportEditorStateDto>>,
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
			editor_state: Arc::new(Mutex::new(ViewportEditorStateDto::default())),
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
				Arc::clone(&self.editor_state),
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

	pub fn editor_state(&self) -> Result<ViewportEditorStateDto, String> {
		self.editor_state
			.lock()
			.map(|state| state.clone())
			.map_err(|_| "Viewport editor state lock is poisoned".into())
	}

	/// Queue ordered boundaries on the native owner without blocking the UI thread.
	pub fn request_action(
		&self,
		action: ViewportActionDto,
	) -> Result<mpsc::Receiver<Result<ViewportEditorStateDto, String>>, String> {
		action.validate()?;
		#[cfg(windows)]
		{
			// Capture the actual workspace before entering lifecycle/queue locks.
			// The worker rechecks this identity under its mutation lock.
			let workspace_generation = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?
				.as_ref()
				.ok_or("Open a project before using viewport tools")?
				.generation;
			let running = self
				.running
				.lock()
				.map_err(|_| "Viewport lifecycle lock is poisoned")?;
			let running = running
				.as_ref()
				.ok_or("Open a project and attach its viewport first")?;
			let (reply, receiver) = mpsc::sync_channel(1);
			running
				.control
				.request_action(action, workspace_generation, reply)?;
			Ok(receiver)
		}
		#[cfg(not(windows))]
		{
			let _ = action;
			Err("The embedded Vulkan editor currently supports Windows only".into())
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
	use crate::viewport_interaction::ViewportInteraction;
	use bridge::editor::{ViewportInputDto, ViewportModifiersDto};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle};
	use renderer::{vulkan::CameraUniform, DrawStatus, WindowRenderer};
	use runtime::{assets::AssetManager, scene::Scene, scene_io, Camera};
	use std::{
		collections::VecDeque,
		num::NonZeroIsize,
		path::PathBuf,
		thread::JoinHandle,
		time::{Duration, Instant},
	};
	use tauri::Emitter;
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
	};
	use winit::{
		application::ApplicationHandler,
		dpi::{PhysicalPosition, PhysicalSize},
		event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
		event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
		keyboard::{ModifiersState, PhysicalKey},
		platform::windows::EventLoopBuilderExtWindows,
		window::{CursorGrabMode, Window, WindowId},
	};

	const FRAME: Duration = Duration::from_nanos(16_666_667);
	const EDITOR_STATE_INTERVAL: Duration = Duration::from_millis(50);

	fn show_viewport_window(window: &Window) -> Result<(), String> {
		let handle = window.window_handle().map_err(|error| error.to_string())?;
		let RawWindowHandle::Win32(handle) = handle.as_raw() else {
			return Err("Native viewport window has no Win32 handle".into());
		};
		window.set_visible(true);
		// Win32 creates child windows below existing siblings. Wry's windowed
		// WebView2 fills the same parent, so showing the viewport is insufficient.
		// SAFETY: The worker owns the live child HWND on this thread.
		let positioned = unsafe {
			SetWindowPos(
				handle.hwnd.get(),
				HWND_TOP,
				0,
				0,
				0,
				0,
				SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
			)
		};
		if positioned == 0 {
			return Err(format!(
				"Could not show the Vulkan viewport above the editor WebView: {}",
				std::io::Error::last_os_error()
			));
		}
		Ok(())
	}

	fn pending_editor_publication(
		current_revision: u64,
		published_revision: u64,
		last_emit: Instant,
	) -> Option<Instant> {
		(current_revision != published_revision).then_some(last_emit + EDITOR_STATE_INTERVAL)
	}
	struct ActionRequest {
		action: ViewportActionDto,
		workspace_generation: u64,
		reply: mpsc::SyncSender<Result<ViewportEditorStateDto, String>>,
		deadline: Instant,
	}
	impl ActionRequest {
		fn validate_context(&self, workspace_generation: u64, now: Instant) -> Result<(), String> {
			if now > self.deadline {
				return Err("Viewport action expired before it could be applied".into());
			}
			if self.workspace_generation != workspace_generation {
				return Err("Viewport action belongs to a different open project; retry in the current workspace".into());
			}
			Ok(())
		}
	}
	#[cfg(test)]
	mod action_context_tests {
		use super::*;

		fn request(generation: u64, deadline: Instant) -> ActionRequest {
			let (reply, _) = mpsc::sync_channel(1);
			ActionRequest {
				action: ViewportActionDto::SetGrid { visible: true },
				workspace_generation: generation,
				reply,
				deadline,
			}
		}

		#[test]
		fn queued_action_rejects_a_replaced_workspace() {
			let now = Instant::now();
			let queued = request(41, now + Duration::from_secs(8));
			assert!(queued.validate_context(41, now).is_ok());
			assert!(queued
				.validate_context(42, now)
				.unwrap_err()
				.contains("different open project"));
		}

		#[test]
		fn action_expiry_is_rechecked_after_waiting_for_the_workspace() {
			let now = Instant::now();
			let queued = request(41, now + Duration::from_secs(8));
			assert!(queued
				.validate_context(41, now + Duration::from_secs(7))
				.is_ok());
			assert!(queued
				.validate_context(41, now + Duration::from_secs(9))
				.unwrap_err()
				.contains("expired"));
		}

		#[test]
		fn final_editor_publication_wakes_once_then_returns_to_idle() {
			let emitted = Instant::now();
			// A completion immediately after CommitPlacement still needs an
			// event-loop wake even when there is no further pointer/frame work.
			assert_eq!(
				pending_editor_publication(8, 7, emitted),
				Some(emitted + Duration::from_millis(50))
			);
			assert_eq!(
				pending_editor_publication(8, 8, emitted + Duration::from_millis(50)),
				None
			);
		}
	}

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
		actions: Mutex<VecDeque<ActionRequest>>,
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
				actions: Mutex::new(VecDeque::new()),
			})
		}
		pub(super) fn request_action(
			&self,
			action: ViewportActionDto,
			workspace_generation: u64,
			reply: mpsc::SyncSender<Result<ViewportEditorStateDto, String>>,
		) -> Result<(), String> {
			if self.stopping() {
				return Err("Viewport is shutting down".into());
			}
			let mut actions = self
				.actions
				.lock()
				.map_err(|_| "Viewport action lock is poisoned")?;
			if actions.len() >= 64 {
				return Err("Viewport action queue is full; coalesce continuous input".into());
			}
			actions.push_back(ActionRequest {
				action,
				workspace_generation,
				reply,
				deadline: Instant::now() + Duration::from_secs(8),
			});
			drop(actions);
			self.wake();
			Ok(())
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
		editor_state: Arc<Mutex<ViewportEditorStateDto>>,
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
						editor_state,
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
		editor_state: Arc<Mutex<ViewportEditorStateDto>>,
		interaction: ViewportInteraction,
		last_editor_emit: Instant,
		last_editor_revision: u64,
		bounds: ViewportBoundsDto,
		attached: bool,
		parent_visible: bool,
		dpi: f64,
		dirty: bool,
		occluded: bool,
		failed: bool,
		last_tick: Option<Instant>,
		last_camera_tick: Option<Instant>,
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
			editor_state: Arc<Mutex<ViewportEditorStateDto>>,
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
				editor_state,
				interaction: ViewportInteraction::default(),
				last_editor_emit: now - Duration::from_secs(1),
				last_editor_revision: u64::MAX,
				bounds: ViewportBoundsDto::default(),
				attached: false,
				parent_visible: true,
				dpi: 1.0,
				dirty: true,
				occluded: false,
				failed: false,
				last_tick: None,
				last_camera_tick: None,
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
			self.cancel_editor();
			if let Some(window) = self.window.as_ref() {
				let _ = window.set_cursor_grab(CursorGrabMode::None);
			}
			if let Some(window) = self.window.as_ref() {
				window.set_visible(false);
			}
			self.dirty = false;
			self.last_tick = None;
			self.prepared_extent = None;
			self.last_status = Instant::now() - Duration::from_secs(1);
			if let Ok(mut status) = self.status.lock() {
				status.error = Some(error.into());
				status.visible = false;
				status.width = 0;
				status.height = 0;
			}
		}
		fn update_window_visibility(&mut self) {
			if let Some(window) = self.window.as_ref() {
				if self.active() && !self.failed {
					if let Err(error) = show_viewport_window(window) {
						self.fail("window_z_order", error);
					}
				} else {
					window.set_visible(false);
				}
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
		fn publish_editor(&mut self, force: bool) {
			let current = self.interaction.state();
			if let Ok(mut state) = self.editor_state.lock() {
				*state = current.clone();
			}
			let now = Instant::now();
			if current.revision != self.last_editor_revision
				&& (force || now.duration_since(self.last_editor_emit) >= EDITOR_STATE_INTERVAL)
			{
				let _ = self.parent.emit("editor://viewport-editor-state", &current);
				self.last_editor_revision = current.revision;
				self.last_editor_emit = now;
			}
			if self.interaction.take_document_changed() {
				let payload = self.shared.lock().ok().and_then(|workspace| workspace.as_ref().map(|workspace| serde_json::json!({
					"workspace_generation": workspace.generation, "revision": workspace.backend.session().revision()
				})));
				if let Some(payload) = payload {
					let _ = self
						.parent
						.emit("editor://viewport-document-changed", payload);
				}
			}
		}
		fn update_capture(&self) {
			if let Some(window) = self.window.as_ref() {
				let captured = self.interaction.captured();
				let _ = window.set_cursor_grab(if captured {
					CursorGrabMode::Confined
				} else {
					CursorGrabMode::None
				});
			}
		}
		fn cancel_editor(&mut self) {
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					if let Err(error) = self.interaction.cancel(workspace) {
						self.interaction.report(error);
					}
				}
			}
			self.update_capture();
			self.publish_editor(true);
		}
		fn process_actions(&mut self) {
			let requests: Vec<_> = self
				.control
				.actions
				.lock()
				.map(|mut actions| actions.drain(..).collect())
				.unwrap_or_default();
			for request in requests {
				if Instant::now() > request.deadline {
					let _ = request.reply.send(Err(
						"Viewport action expired before it could be applied".into(),
					));
					continue;
				}
				let reply = request.reply.clone();
				let extent = self.prepared_extent.unwrap_or_else(|| self.extent());
				let result = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned".to_string())
					.and_then(|mut workspace| {
						let workspace = workspace
							.as_mut()
							.ok_or("Open a project before using viewport tools")?;
						// Waiting for the workspace lock can cross both project and
						// timeout boundaries; reject before any action can mutate it.
						request.validate_context(workspace.generation, Instant::now())?;
						self.interaction.action(request.action, workspace, extent)
					});
				if let Err(error) = result.as_ref() {
					self.interaction.report(error.clone());
				}
				self.dirty = true;
				self.update_capture();
				self.publish_editor(true);
				let _ = reply.send(result.map(|()| self.interaction.state()));
			}
		}
		fn process_input(
			&mut self,
			kind: &str,
			button: Option<String>,
			code: Option<String>,
			repeat: bool,
			delta: Option<[f64; 2]>,
		) {
			let extent = self.prepared_extent.unwrap_or_else(|| self.extent());
			if extent.contains(&0) {
				return;
			}
			let position = self.window.as_ref().map(|window| {
				let size = window.inner_size();
				[
					self.cursor[0] * extent[0] as f64 / size.width.max(1) as f64,
					self.cursor[1] * extent[1] as f64 / size.height.max(1) as f64,
				]
			});
			let input = ViewportInputDto {
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
			let before = self.interaction.state().revision;
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					if let Err(error) = self.interaction.input(&input, workspace, extent, self.dpi)
					{
						let _ = self.interaction.cancel(workspace);
						self.interaction.report(error);
					}
				}
			}
			self.dirty |= before != self.interaction.state().revision
				|| kind == "pointer_move" && self.interaction.captured();
			self.update_capture();
			self.publish_editor(kind != "pointer_move");
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
			let retrying = self.failed && (invalidated || changed);
			if self.attached != attached || retrying {
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
				self.cancel_editor();
			}
			if changed {
				let extent = self.extent();
				if let Ok(mut workspace) = self.shared.lock() {
					if let Some(workspace) = workspace.as_mut() {
						if let Err(error) =
							self.interaction.geometry_changed(workspace, extent, dpi)
						{
							self.interaction.report(error);
						}
					}
				}
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
					if let Some(gpu) = self.gpu.as_mut() {
						gpu.renderer.resize(width, height);
					}
				}
			}
			// A failed child was explicitly hidden. Retrying with identical
			// bounds must show it again even when no resize is required.
			if changed || retrying {
				self.update_window_visibility();
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
				status.visible = self.active() && !self.failed;
				status.width = extent[0];
				status.height = extent[1];
			}
			if changed {
				self.last_status = Instant::now() - Duration::from_secs(1);
			}
			self.process_actions();
		}

		fn draw(&mut self) -> Result<(), String> {
			if !self.active() {
				return Ok(());
			}
			let packet = loop {
				let captured = {
					let mut workspace = self
						.shared
						.lock()
						.map_err(|_| "Editor document lock is poisoned")?;
					let workspace = workspace
						.as_mut()
						.ok_or("Open a scene before attaching a viewport")?;
					self.interaction.sync(workspace)?;
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
			if self.interaction.settings.view == bridge::viewport::ViewportView::Scene {
				self.interaction.synchronize_targets(
					&gpu.scene,
					generation,
					revision,
					[width, height],
				)?;
			}
			let uniform = if self.interaction.settings.view == bridge::viewport::ViewportView::Scene
			{
				CameraUniform::from_view_projection(
					self.interaction
						.camera
						.view_projection([width, height], self.interaction.settings.projection)?,
				)
			} else {
				CameraUniform::perspective(
					gpu.camera.position,
					gpu.camera.target(),
					[0.0, 1.0, 0.0],
					gpu.camera.vertical_fov,
					width as f32 / height as f32,
					gpu.camera.near,
					gpu.camera.far,
				)
			};
			let visuals = {
				let workspace = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				self.interaction.visuals(
					workspace.as_ref().ok_or("No scene is open")?,
					[width, height],
					self.dpi,
				)?
			};
			gpu.renderer.set_editor_visuals(&visuals)?;
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
			self.publish_editor(false);
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
			.with_active(false)
			.with_visible(false)
			.with_inner_size(PhysicalSize::new(width.max(1), height.max(1)))
			.with_position(PhysicalPosition::new(
				(self.bounds.x * self.dpi).round() as i32,
				(self.bounds.y * self.dpi).round() as i32,
			));
			match event_loop.create_window(attributes) {
				Ok(window) => {
					self.window = Some(Arc::new(window));
					self.dirty = true;
					self.update_window_visibility();
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
					self.cancel_editor();
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
						self.cancel_editor();
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
					self.process_input("pointer_leave", None, None, false, None);
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
					// Release is an ordered boundary; flush the newest pointer value first.
					if self.pending_motion.take().is_some() {
						self.process_input("pointer_move", None, None, false, None);
					}
					if state == ElementState::Pressed {
						if let Some(window) = self.window.as_ref() {
							window.focus_window();
						}
					}
					self.process_input(
						if state == ElementState::Pressed {
							"pointer_down"
						} else {
							"pointer_up"
						},
						Some(button.clone()),
						None,
						false,
						None,
					);
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
					self.process_input(kind, None, None, false, Some(delta));
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
					self.process_input(
						if event.state == ElementState::Pressed {
							"key_down"
						} else {
							"key_up"
						},
						None,
						Some(code.clone()),
						event.repeat,
						None,
					);
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
					self.process_input(
						if focused { "focused" } else { "blurred" },
						None,
						None,
						false,
						None,
					);
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
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					let before = workspace.backend.session().revision();
					if let Err(error) = self.interaction.sync(workspace) {
						self.interaction.report(error);
					}
					self.interaction.drain_imports(workspace);
					self.dirty |= before != workspace.backend.session().revision();
				}
			}
			if self.pending_motion.is_some() && now.duration_since(self.last_motion) >= FRAME {
				self.pending_motion = None;
				self.last_motion = now;
				self.process_input("pointer_move", None, None, false, None);
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
			let camera_ticking =
				self.interaction.needs_camera_tick() && self.active() && !self.failed;
			let camera_advanced = camera_ticking && now >= self.next_frame;
			if camera_advanced {
				let elapsed = self
					.last_camera_tick
					.replace(now)
					.map_or(FRAME.as_secs_f32(), |last| {
						now.duration_since(last).as_secs_f32()
					});
				self.interaction.camera_tick(elapsed);
				self.dirty = true;
			}
			if !camera_ticking {
				self.last_camera_tick = None;
			}
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
				if camera_advanced {
					self.next_frame = now + FRAME;
				} else if !camera_ticking {
					self.next_frame = now;
				}
			}
			self.publish_editor(false);
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
			let mut deadline = if ticking || camera_ticking {
				Some(self.next_frame)
			} else {
				None
			};
			if self.pending_motion.is_some() {
				let motion = self.last_motion + FRAME;
				deadline = Some(deadline.map_or(motion, |deadline| deadline.min(motion)));
			}
			if let Some(publication) = pending_editor_publication(
				self.interaction.state().revision,
				self.last_editor_revision,
				self.last_editor_emit,
			) {
				deadline = Some(deadline.map_or(publication, |deadline| deadline.min(publication)));
			}
			if self.dirty && self.active() && !self.failed {
				if let Some(retry) = self.redraw_retry {
					deadline = Some(deadline.map_or(retry, |deadline| deadline.min(retry)));
				}
			}
			event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
		}
		fn exiting(&mut self, _: &ActiveEventLoop) {
			self.cancel_editor();
			if let Ok(mut actions) = self.control.actions.lock() {
				for request in actions.drain(..) {
					let _ = request
						.reply
						.send(Err("Viewport stopped before the action was applied".into()));
				}
			}
			self.gpu = None;
			self.window = None;
			engine_logger::debug!(target: "editor_viewport", "Viewport worker teardown completed");
		}
	}
}
