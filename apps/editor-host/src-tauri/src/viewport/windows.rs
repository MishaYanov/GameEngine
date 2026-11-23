use super::*;

mod draw;
mod mailbox;
mod native_window;
mod play;
mod prop_preview;

use crate::viewport_interaction::{GameCameraCapture, ViewportInteraction};
use bridge::editor::{
	ViewportInputDto, ViewportModifiersDto, ViewportSceneLoadDto, ViewportSceneLoadStage,
};
pub(super) use mailbox::Control;
use native_window::KeyboardFocusHook;
use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle};
use renderer::{vulkan::CameraUniform, DrawStatus, WindowRenderer};
use runtime::camera_input::{
	normalize_mouse, normalize_wheel, CameraInputPreferences, CameraIntent, WheelSample,
};
use runtime::scene_load_timing::{Outcome as LoadOutcome, SceneLoadTiming, Scope as LoadScope};
use runtime::{
	assets::AssetManager, scene::Scene, scene_catalog::SceneId, scene_editor::PreparedScene,
	scene_io, Camera, GameContentRect,
};
use std::{
	collections::VecDeque,
	mem::ManuallyDrop,
	num::NonZeroIsize,
	path::PathBuf,
	sync::atomic::{AtomicBool, Ordering},
	thread::JoinHandle,
	time::{Duration, Instant},
};
use tauri::Emitter;
use windows_sys::Win32::{
	Foundation::{HWND, LPARAM, LRESULT, WPARAM},
	UI::{
		Input::KeyboardAndMouse::{GetFocus, SetFocus},
		Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
		WindowsAndMessaging::{
			GetForegroundWindow, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
			SWP_NOOWNERZORDER, SWP_NOSIZE, WM_KILLFOCUS, WM_NCDESTROY,
		},
	},
};
use winit::{
	application::ApplicationHandler,
	dpi::{PhysicalPosition, PhysicalSize},
	event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
	keyboard::{KeyCode, ModifiersState, PhysicalKey},
	platform::windows::EventLoopBuilderExtWindows,
	window::{CursorGrabMode, Window, WindowId},
};

const FRAME: Duration = Duration::from_nanos(16_666_667);
const EDITOR_STATE_INTERVAL: Duration = Duration::from_millis(50);
const SCENE_LOAD_INTERVAL: Duration = Duration::from_millis(75);
const STATUS_INTERVAL: Duration = Duration::from_millis(500);

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

/// Both checks run under the workspace lock, including after GPU preparation.
/// Obsolete automatic transitions are discarded; an explicit restart must fail visibly.
enum PlayInstall<'a> {
	Transition {
		generation: u64,
		target: &'a SceneId,
		run_id: &'a str,
		scene_generation: u64,
	},
	Restart(&'a ActionRequest),
}
impl PlayInstall<'_> {
	fn validate(&self, workspace: &EditorWorkspace, now: Instant) -> Result<bool, String> {
		match self {
			Self::Transition {
				generation,
				target,
				run_id,
				scene_generation,
			} => Ok(workspace.generation == *generation
				&& workspace.backend.pending_play_scene() == Some(*target)
				&& workspace
					.backend
					.session()
					.owns_play_scene(run_id, *scene_generation)),
			Self::Restart(request) => {
				request.validate_context(workspace.generation, now)?;
				let ViewportActionDto::RestartRun {
					run_id,
					expected_revision,
				} = &request.action
				else {
					return Err("Expected a Restart Run request".into());
				};
				workspace
					.backend
					.validate_restart_play(run_id, *expected_revision)
					.map_err(|error| error.to_string())?;
				Ok(true)
			}
		}
	}
}
#[cfg(test)]
#[path = "../../tests/unit/viewport/windows/action_context_tests.rs"]
mod action_context_tests;

#[derive(Clone, Copy)]
struct Wake;

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
			control.cancel_asset_changes();
		})
		.map_err(|error| {
			engine_logger::error!(target: "editor_viewport", "Viewport worker failed: operation=thread_start");
			format!("Failed to start embedded viewport worker: {error}")
		})
}

// Order matters: uploaded models/assets retire before the renderer/device.
struct GpuState {
	scene: Scene,
	prop_preview: Option<prop_preview::PropGpuPreview>,
	particle_draw: Option<runtime::particle_draw::ParticleDraw>,
	assets: ManuallyDrop<AssetManager>,
	renderer: WindowRenderer,
	asset_root: PathBuf,
	revision: u64,
	generation: u64,
	change_cursor: u64,
	assets_invalidated: bool,
}

struct RenderPacket {
	warmed: crate::editor::scene_warming::WarmClaim,
	warm_requested: bool,
	warming: crate::editor::scene_warming::WarmingContext,
	animations: Option<runtime::animation::AnimationBank>,
	animation_preview: Option<(runtime::EntityKey, Arc<runtime::animation::AnimationPose>)>,
	document: Option<Arc<runtime::SceneData>>,
	game_camera: Option<Camera>,
	revision: u64,
	generation: u64,
	scene_id: Option<runtime::scene_catalog::SceneId>,
	switch_clock: Option<(u64, Instant, Instant)>,
	change_cursor: u64,
	asset_root: PathBuf,
	lights_changed: bool,
}

fn publish_scene_load(
	parent: &tauri::WebviewWindow,
	control: &Control,
	shared: &Mutex<ViewportStatusDto>,
	progress: ViewportSceneLoadDto,
) {
	let event = shared.lock().ok().and_then(|mut status| {
		if !status.attached || status.scene_load.as_ref() == Some(&progress) {
			return None;
		}
		status.scene_load = Some(progress);
		Some(status.clone())
	});
	if let Some(event) = event {
		let _ = parent.emit("editor://viewport-status", control.requested_status(event));
	}
}

/// Refresh readiness without announcing another load on ordinary authored edits.
fn advance_presented_revision(
	status: &mut ViewportStatusDto,
	generation: u64,
	scene_id: Option<&runtime::scene_catalog::SceneId>,
	revision: u64,
) -> bool {
	let Some(progress) = status.scene_load.as_mut() else {
		return false;
	};
	if progress.stage != ViewportSceneLoadStage::Presented
		|| progress.workspace_generation != generation
		|| scene_id != Some(&progress.scene_id)
		|| revision <= progress.scene_revision
	{
		return false;
	}
	progress.scene_revision = revision;
	true
}
impl Drop for GpuState {
	fn drop(&mut self) {
		// This is the only path that takes the asset owner. Automatic field
		// destruction cannot release it again after an uncertain GPU wait.
		let assets = unsafe { ManuallyDrop::take(&mut self.assets) };
		if self
			.renderer
			.retire_resources((
				std::mem::take(&mut self.scene),
				self.prop_preview.take(),
				self.particle_draw.take(),
				assets,
			))
			.is_err()
		{
			engine_logger::error!(target: "editor_viewport", "Viewport teardown failed: operation=gpu_wait_idle, generation={}", self.generation);
		}
		engine_logger::debug!(target: "editor_viewport", "Viewport GPU state retired: generation={}", self.generation);
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthoredLoadKey {
	generation: u64,
	revision: u64,
	scene_id: SceneId,
}

impl AuthoredLoadKey {
	fn matches(&self, workspace: &EditorWorkspace) -> bool {
		let session = workspace.backend.session();
		workspace.generation == self.generation
			&& session.scene_revision() == self.revision
			&& session.active_scene_id() == &self.scene_id
			&& !session.is_playing()
	}
}

struct AuthoredLoad {
	key: AuthoredLoadKey,
	timing: SceneLoadTiming,
	warming: Option<(
		crate::editor::scene_warming::WarmingContext,
		bridge::viewport::WarmingSessionRowDto,
	)>,
}

struct Worker {
	particle_samples: Vec<runtime::particle_draw::ParticleSample>,
	scene_audio: runtime::scene_audio::NativeSceneAudio,
	warm_measurement: bool,
	authored_load: Option<AuthoredLoad>,
	last_authored_present: Option<(u64, SceneId)>,
	play_load: Option<(u64, String, u64, SceneLoadTiming)>,
	gpu: Option<GpuState>,
	prop_navigation: Option<prop_preview::PropNavigation>,
	focus_hook: Option<KeyboardFocusHook>,
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
	game_keyboard_active: bool,
	game_focus_pending: bool,
	game_camera_capture: Option<GameCameraCapture>,
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
	pending_fly_motion: [f64; 2],
	last_motion: Instant,
	last_status: Instant,
	status_pending: bool,
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
			particle_samples: Vec::new(),
			scene_audio: Default::default(),
			warm_measurement: std::env::var_os("ENGINE_EDITOR_WARM_MEASURE").as_deref()
				== Some(std::ffi::OsStr::new("1")),
			authored_load: None,
			last_authored_present: None,
			play_load: None,
			gpu: None,
			prop_navigation: None,
			focus_hook: None,
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
			game_keyboard_active: false,
			game_focus_pending: false,
			game_camera_capture: None,
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
			pending_fly_motion: [0.0; 2],
			last_motion: now - FRAME,
			last_status: now - Duration::from_secs(1),
			status_pending: false,
		}
	}
	fn fail(&mut self, operation: &'static str, error: impl Into<String>) {
		if !self.failed {
			engine_logger::error!(target: "editor_viewport", "Viewport entered failed state: operation={operation}");
		}
		self.failed = true;
		self.release_keyboard_focus();
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
	fn active(&self) -> bool {
		self.attached
			&& self.parent_visible
			&& self.bounds.visible
			&& self.bounds.width > 0.0
			&& self.bounds.height > 0.0
			&& !self.occluded
	}
	fn flush_fly_motion(&mut self) {
		let delta = std::mem::take(&mut self.pending_fly_motion);
		if delta == [0.0; 2] {
			return;
		}
		self.sync_workspace_interaction(false);
		if self.interaction.is_flying() && self.native_keyboard_focused() {
			self.interaction.look_from_fly_motion(delta);
			self.dirty = true;
		}
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
			let payload = self.shared.lock().ok().and_then(|workspace| {
				workspace.as_ref().map(|workspace| serde_json::json!({
				"workspace_generation": workspace.generation, "revision": workspace.backend.session().revision()
			}))
			});
			if let Some(payload) = payload {
				let _ = self
					.parent
					.emit("editor://viewport-document-changed", payload);
			}
		}
	}
	fn update_capture(&self) {
		if self.game_camera_capture.is_some() {
			return;
		}
		if let Some(window) = self.window.as_ref() {
			let captured = self.prop_navigation.as_ref().map_or_else(
				|| self.interaction.captured(),
				prop_preview::PropNavigation::captured,
			);
			let _ = window.set_cursor_grab(if captured {
				CursorGrabMode::Confined
			} else {
				CursorGrabMode::None
			});
		}
	}
	fn release_ended_scene_capture(&mut self, was_captured: bool) {
		if was_captured && !self.interaction.captured() {
			self.pending_fly_motion = [0.0; 2];
			self.update_capture();
		}
	}
	fn sync_workspace_interaction(&mut self, drain_imports: bool) {
		let was_captured = self.interaction.captured();
		if let Ok(mut slot) = self.shared.lock() {
			if let Some(workspace) = slot.as_mut() {
				let before = workspace.backend.session().revision();
				if let Err(error) = self.interaction.sync(workspace) {
					self.interaction.report(error);
				}
				if drain_imports {
					self.interaction.drain_imports(workspace);
				}
				self.dirty |= before != workspace.backend.session().revision();
			}
		}
		self.release_ended_scene_capture(was_captured);
	}
	fn camera_surface_focused(&self) -> bool {
		self.active()
			&& !self.failed
			&& !self.control.keyboard_blurred.load(Ordering::Acquire)
			&& self.native_keyboard_focused()
			&& self.interaction.keyboard_focused()
			&& self.interaction.game_keyboard_active()
			&& self
				.control
				.latest
				.lock()
				.is_ok_and(|latest| !latest.clear_input && !latest.stop)
	}
	fn acquire_game_camera(&mut self) {
		if self.game_camera_capture.is_some() || !self.camera_surface_focused() {
			return;
		}
		let Some(window) = self.window.as_ref() else {
			return;
		};
		let result = {
			let Ok(slot) = self.shared.lock() else { return };
			let Some(workspace) = slot.as_ref() else {
				return;
			};
			GameCameraCapture::acquire(
				&self.interaction,
				workspace,
				self.camera_surface_focused(),
				|| {
					window
						.set_cursor_grab(CursorGrabMode::Locked)
						.or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
						.map_err(|err| {
							format!("Could not capture the Game camera. Click the viewport to retry: {err}")
						})
				},
			)
		};
		match result {
			Ok(Some(capture)) => {
				self.pending_motion = None;
				self.game_camera_capture = Some(capture);
				self.interaction.clear_error();
				window.set_cursor_visible(false);
			}
			Err(message) => self.interaction.report(message),
			Ok(None) => {}
		}
	}
	fn release_game_camera(&mut self) {
		let Some(capture) = self.game_camera_capture.take() else {
			return;
		};
		if let Ok(mut slot) = self.shared.lock() {
			if let Some(workspace) = slot.as_mut().filter(|w| capture.current(w)) {
				self.interaction.clear_play_input(workspace);
			}
		}
		if let Some(window) = &self.window {
			let result = window.set_cursor_grab(CursorGrabMode::None);
			window.set_cursor_visible(true);
			if let Err(error) = result {
				self.interaction
					.report(format!("Could not release the Game camera cursor: {error}"));
			}
		}
	}
	fn refresh_game_camera(&mut self) {
		let Some(capture) = self.game_camera_capture.as_ref() else {
			return;
		};
		if !self.camera_surface_focused()
			|| !self.shared.lock().is_ok_and(|slot| {
				slot.as_ref()
					.is_some_and(|workspace| capture.current(workspace))
			}) {
			self.release_game_camera();
		}
	}
	fn submit_game_camera(&mut self, normalized: Result<CameraIntent, String>) {
		let Some(capture) = self.game_camera_capture.as_ref() else {
			return;
		};
		let intent = match normalized {
			Ok(intent) => intent,
			Err(message) => {
				self.interaction.report(message);
				return;
			}
		};
		// Run/scene are checked inside the same workspace lock as submission.
		let accepted = self.shared.lock().is_ok_and(|mut slot| {
			self.camera_surface_focused()
				&& slot
					.as_mut()
					.is_some_and(|workspace| capture.submit(workspace, intent).is_ok())
		});
		if !accepted {
			self.release_game_camera();
		}
	}
	fn cancel_editor(&mut self) {
		if let Some(navigation) = &mut self.prop_navigation {
			self.dirty |= navigation.captured();
			navigation.cancel();
		}
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
			let result = if matches!(&request.action, ViewportActionDto::RestartRun { .. }) {
				self.restart_run(&request)
			} else {
				self.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned".to_string())
					.and_then(|mut workspace| {
						let workspace = workspace
							.as_mut()
							.ok_or("Open a project before using viewport tools")?;
						// Waiting for the workspace lock can cross both project and
						// timeout boundaries; reject before any action can mutate it.
						request.validate_context(workspace.generation, Instant::now())?;
						if workspace.prop_preview_active()
							&& !matches!(
								request.action,
								ViewportActionDto::Cancel | ViewportActionDto::CancelPlacement
							) {
							return Err("Close the Prop preview to use Scene tools".into());
						}
						self.interaction
							.action(request.action.clone(), workspace, extent)
					})
			};
			if let Err(error) = result.as_ref() {
				// A late failure must not paint an error onto another project or a newer run.
				let current = self.shared.lock().ok().is_some_and(|shared| {
					shared.as_ref().is_some_and(|workspace| {
						workspace.generation == request.workspace_generation
							&& match &request.action {
								ViewportActionDto::RestartRun { run_id, .. } => workspace
									.backend
									.play_status()
									.is_some_and(|status| status.run_id == *run_id),
								_ => true,
							}
					})
				});
				if current {
					self.interaction.report(error.clone());
				}
			}
			self.dirty = true;
			self.refresh_game_keyboard_focus();
			self.refresh_game_camera();
			self.update_capture();
			self.publish_editor(true);
			let _ = reply.send(result.map(|()| self.interaction.state()));
		}
	}
	fn process_asset_changes(&mut self) {
		let requests: Vec<_> = self
			.control
			.asset_changes
			.lock()
			.map(|mut requests| requests.drain(..).collect())
			.unwrap_or_default();
		for request in requests {
			let reply = request.reply.clone();
			let result = self.apply_asset_change(request);
			self.dirty = true;
			let _ = reply.send(result);
		}
	}
	fn apply_asset_change(
		&mut self,
		request: crate::asset_reimport_gpu::AssetChangeRequest,
	) -> crate::asset_reimport_gpu::AssetChangeResult {
		{
			let shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let workspace = shared
				.as_ref()
				.ok_or("Open a project before changing assets")?;
			request.validate(workspace)?;
			if self.gpu.as_ref().is_some_and(|gpu| {
				gpu.generation != workspace.generation || gpu.asset_root != workspace.asset_root
			}) {
				return Err(
					"Viewport is synchronizing the current project; retry asset publication".into(),
				);
			}
		}
		let prepared = if let Some(gpu) = self.gpu.as_mut() {
			let candidate = gpu
				.assets
				.prepare_gpu_asset(&gpu.renderer, &request.change)
				.map_err(|error| error.to_string())?;
			// Dropping cache owners must not race a submitted frame. This wait
			// and all uploads run outside the authored workspace lock.
			if let Err(error) = gpu.renderer.wait_idle() {
				gpu.assets.discard_gpu_asset(candidate);
				return Err(error.to_string());
			}
			Some(candidate)
		} else {
			None
		};
		let mut prepared = prepared;
		let result = (|| {
			let mut shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let workspace = shared
				.as_mut()
				.ok_or("Open a project before changing assets")?;
			request.validate(workspace)?;
			crate::asset_reimport_gpu::publish(request.change, workspace, || {
				if let (Some(gpu), Some(candidate)) = (self.gpu.as_mut(), prepared.take()) {
					gpu.assets.promote_gpu_asset(candidate);
					gpu.assets_invalidated = true;
				}
			})
		})();
		if let (Some(gpu), Some(candidate)) = (self.gpu.as_mut(), prepared) {
			gpu.assets.discard_gpu_asset(candidate);
		}
		result
	}
	/// Map a physical window position to the actual surface pixels.
	fn raw_input_surface(&self) -> (Option<[f64; 2]>, [u32; 2]) {
		let extent = self.prepared_extent.unwrap_or([0; 2]);
		let point = self.window.as_ref().and_then(|window| {
			let size = window.inner_size();
			(size.width > 0 && size.height > 0 && !extent.contains(&0)).then(|| {
				[
					self.cursor[0] * f64::from(extent[0]) / f64::from(size.width),
					self.cursor[1] * f64::from(extent[1]) / f64::from(size.height),
				]
			})
		});
		(point, extent)
	}
	/// Map surface pixels to the same Game content rectangle used by the camera.
	fn input_surface(&self) -> (Option<[f64; 2]>, [u32; 2]) {
		let (point, extent) = self.raw_input_surface();
		if self.interaction.settings.view == bridge::viewport::ViewportView::Game
			|| self.interaction.preview_active()
		{
			if let Some(rect) = GameContentRect::fit(extent[0], extent[1]) {
				return (point.and_then(|p| rect.local_pointer(p)), rect.extent());
			}
		}
		(point, extent)
	}

	fn process_input(
		&mut self,
		kind: &str,
		button: Option<String>,
		code: Option<String>,
		repeat: bool,
		delta: Option<[f64; 2]>,
	) {
		if self.process_prop_input(kind, button.as_deref(), code.as_deref(), repeat, delta) {
			return;
		}
		let (position, extent) = self.input_surface();
		if extent.contains(&0) && !matches!(kind, "focused" | "blurred") {
			return;
		}
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
		let before = self.interaction.revision();
		if let Ok(mut workspace) = self.shared.lock() {
			if let Some(workspace) = workspace.as_mut() {
				if let Err(error) = self.interaction.input(&input, workspace, extent, self.dpi) {
					let _ = self.interaction.cancel(workspace);
					self.interaction.report(error);
				}
			}
		}
		self.dirty |= before != self.interaction.revision()
			|| kind == "pointer_move" && self.interaction.captured()
			|| matches!(kind, "key_down" | "key_up") && self.interaction.game_keyboard_active();
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
		self.drain_native_blur();
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
					latest.sky_refresh_generation.take(),
				);
				latest.invalidated = false;
				latest.wake_queued = false;
				latest.clear_input = false;
				values
			})
			.map_err(|_| ());
		let Ok((
			bounds,
			dpi,
			invalidated,
			attached,
			parent_visible,
			clear_input,
			stop,
			sky_refresh_generation,
		)) = values
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
			self.release_keyboard_focus();
			self.clear_play_input();
			self.cancel_editor();
		}
		if changed {
			let extent = self.extent();
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					if let Err(error) = self.interaction.geometry_changed(workspace, extent, dpi) {
						self.interaction.report(error);
					}
				}
			}
			self.update_capture();
		}
		if changed {
			if let Some(window) = &self.window {
				let [width, height] = self.extent();
				window.set_outer_position(PhysicalPosition::new(
					(bounds.x * dpi).round() as i32,
					(bounds.y * dpi).round() as i32,
				));
				let _ = window.request_inner_size(PhysicalSize::new(width.max(1), height.max(1)));
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
		if self.active()
			&& !self.failed
			&& sky_refresh_generation.is_some_and(|generation| {
				self.shared.lock().is_ok_and(|workspace| {
					workspace
						.as_ref()
						.is_some_and(|workspace| workspace.generation == generation)
				})
			}) {
			self.dirty |= self
				.gpu
				.as_mut()
				.is_some_and(|gpu| gpu.assets.refresh_active_sky());
		}
		if !self.active() {
			self.last_tick = None;
			self.pending_motion = None;
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
		self.process_asset_changes();
		self.refresh_game_keyboard_focus();
	}

	fn emit_input(
		&self,
		kind: &str,
		button: Option<String>,
		code: Option<String>,
		repeat: bool,
		delta: Option<[f64; 2]>,
	) {
		if self.prop_navigation.is_some() {
			return;
		}
		if self.interaction.preview_active() && !matches!(kind, "focused" | "blurred") {
			return;
		}
		let (position, extent) = self.input_surface();
		if self.interaction.settings.view == bridge::viewport::ViewportView::Game
			&& position.is_none()
			&& matches!(kind, "pointer_down" | "wheel_lines" | "wheel_pixels")
		{
			return;
		}
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
				self.refresh_game_keyboard_focus();
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
	fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
		if let DeviceEvent::MouseMotion { delta } = event {
			self.drain_native_blur();
			if self.game_camera_capture.is_some() {
				self.submit_game_camera(normalize_mouse(
					delta.0,
					delta.1,
					CameraInputPreferences::default(),
				));
			} else if self.interaction.is_flying()
				&& self.interaction.keyboard_focused()
				&& self.native_keyboard_focused()
			{
				self.pending_fly_motion[0] += delta.0;
				self.pending_fly_motion[1] += delta.1;
			}
		}
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
		self.drain_native_blur();
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
					self.release_keyboard_focus();
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
				if !self.interaction.is_flying() && self.game_camera_capture.is_none() {
					self.pending_motion = Some(self.cursor);
				}
			}
			WindowEvent::CursorLeft { .. } => {
				self.pending_motion = None;
				self.process_input("pointer_leave", None, None, false, None);
				self.emit_input("pointer_leave", None, None, false, None);
			}
			WindowEvent::MouseInput { state, button, .. } => {
				// An editor IPC command can open preview between two native events.
				// Resolve its ownership before transferring OS focus or flushing fly input.
				self.sync_workspace_interaction(false);
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
				self.flush_fly_motion();
				if state == ElementState::Pressed
					&& matches!(button.as_str(), "left" | "middle" | "right")
					&& !self.interaction.preview_active()
					&& (if self.prop_navigation.is_some() {
						self.raw_input_surface().0.is_some()
					} else {
						self.input_surface().0.is_some()
					}) {
					self.focus_keyboard();
					if button == "left" {
						self.refresh_game_camera();
						self.acquire_game_camera();
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
				if self.game_camera_capture.is_some() {
					let sample = match delta {
						MouseScrollDelta::LineDelta(_, y) => WheelSample::Lines(y as f64),
						MouseScrollDelta::PixelDelta(position) => WheelSample::Pixels(position.y),
					};
					self.submit_game_camera(normalize_wheel(sample));
					return;
				}
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
			WindowEvent::KeyboardInput {
				event,
				is_synthetic: false,
				..
			} if (self.prop_navigation.is_some() || self.interaction.keyboard_focused())
				&& self.native_keyboard_focused() =>
			{
				self.flush_fly_motion();
				let release = event.state == ElementState::Pressed
					&& !event.repeat
					&& event.physical_key == PhysicalKey::Code(KeyCode::Escape);
				let code = match event.physical_key {
					PhysicalKey::Code(code) => format!("{code:?}"),
					PhysicalKey::Unidentified(code) => format!("{code:?}"),
				};
				let kind = if event.state == ElementState::Pressed {
					"key_down"
				} else {
					"key_up"
				};
				let editor_key = (self.interaction.settings.view
					== bridge::viewport::ViewportView::Scene)
					.then(|| code.clone());
				self.process_input(kind, None, Some(code), event.repeat, None);
				if let Some(code) = editor_key {
					self.emit_input(kind, None, Some(code), event.repeat, None);
				}
				if release {
					self.release_keyboard_focus();
				}
			}
			WindowEvent::Focused(false) if !self.native_keyboard_focused() => {
				self.note_keyboard_focus(false);
			}
			_ => {}
		}
	}
	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		if self.control.stopping() {
			event_loop.exit();
			return;
		}
		self.drain_native_blur();
		if self.interaction.keyboard_focused() && !self.native_keyboard_focused() {
			self.note_keyboard_focus(false);
		}
		let now = Instant::now();
		self.sync_workspace_interaction(true);
		self.refresh_game_keyboard_focus();
		self.refresh_game_camera();
		let gamepad_deadline = self.poll_gamepad(now);
		if (self.pending_motion.is_some() || self.pending_fly_motion != [0.0; 2])
			&& now.duration_since(self.last_motion) >= FRAME
		{
			self.last_motion = now;
			if self.pending_motion.take().is_some() {
				self.process_input("pointer_move", None, None, false, None);
				self.emit_input("pointer_move", None, None, false, None);
			}
			self.flush_fly_motion();
		}
		let ticking = self
			.shared
			.lock()
			.ok()
			.and_then(|workspace| {
				workspace.as_ref().map(|workspace| {
					self.gpu_matches(workspace) && workspace.backend.session().needs_tick()
				})
			})
			.unwrap_or(false);
		let ticking = ticking && self.active() && !self.failed;
		let camera_ticking = self.interaction.needs_camera_tick() && self.active() && !self.failed;
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
			if let Err(error) = self.tick_play(elapsed) {
				self.fail("simulation_tick", error);
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
		self.sync_scene_audio();
		self.publish_editor(false);
		let sky_deadline = self
			.gpu
			.as_ref()
			.and_then(|gpu| gpu.assets.sky_update_deadline())
			.filter(|_| self.active() && !self.failed);
		if sky_deadline.is_some_and(|deadline| now >= deadline) {
			self.dirty = true;
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
		if now.duration_since(self.last_status) >= STATUS_INTERVAL {
			let status = self.status.lock().ok().map(|status| status.clone());
			if let Some(status) = status {
				let _ = self.parent.emit(
					"editor://viewport-status",
					self.control.requested_status(status),
				);
			}
			self.last_status = now;
			self.status_pending = false;
		}
		let mut deadline = if ticking || camera_ticking {
			Some(self.next_frame)
		} else {
			None
		};
		if let Some(sky) = sky_deadline.filter(|deadline| *deadline > now) {
			deadline = Some(deadline.map_or(sky, |deadline| deadline.min(sky)));
		}
		if let Some(gamepad) = gamepad_deadline {
			deadline = Some(deadline.map_or(gamepad, |deadline| deadline.min(gamepad)));
		}
		if self.pending_motion.is_some() || self.pending_fly_motion != [0.0; 2] {
			let motion = self.last_motion + FRAME;
			deadline = Some(deadline.map_or(motion, |deadline| deadline.min(motion)));
		}
		if self.status_pending {
			let status = self.last_status + STATUS_INTERVAL;
			deadline = Some(deadline.map_or(status, |deadline| deadline.min(status)));
		}
		if let Some(publication) = pending_editor_publication(
			self.interaction.revision(),
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
		self.note_keyboard_focus(false);
		self.cancel_editor();
		if let Ok(mut actions) = self.control.actions.lock() {
			for request in actions.drain(..) {
				let _ = request
					.reply
					.send(Err("Viewport stopped before the action was applied".into()));
			}
		}
		self.gpu = None;
		self.focus_hook = None;
		self.window = None;
		engine_logger::debug!(target: "editor_viewport", "Viewport worker teardown completed");
	}
}
