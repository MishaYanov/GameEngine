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

	/// Called from an IPC blocking task, never from the UI/event-loop thread.
	/// The native owner prepares GPU resources before the publication boundary.
	pub fn request_asset_change(
		&self,
		change: runtime::assets::project_assets::PreparedAssetChange,
		generation: u64,
		revision: u64,
	) -> crate::asset_reimport_gpu::AssetChangeResult {
		#[cfg(windows)]
		{
			self.reap();
			let control = self
				.running
				.lock()
				.map_err(|_| "Viewport lifecycle lock is poisoned")?
				.as_ref()
				.map(|running| Arc::clone(&running.control));
			if let Some(control) = control {
				let (reply, receiver) = mpsc::sync_channel(1);
				control.request_asset_change(
					crate::asset_reimport_gpu::AssetChangeRequest::new(
						change, generation, revision, reply,
					),
				)?;
				return receiver.recv().map_err(|_| {
					"Native asset worker stopped before completing the operation".to_string()
				})?;
			}
		}
		// No live native owner means there are no GPU caches to replace. Future
		// viewport creation imports the freshly published, validated bytes.
		let mut shared = self
			.shared
			.lock()
			.map_err(|_| "Editor document lock is poisoned")?;
		let workspace = shared
			.as_mut()
			.ok_or("Open a project before changing assets")?;
		crate::asset_reimport_gpu::validate_context(workspace, &change, generation, revision)?;
		crate::asset_reimport_gpu::publish(change, workspace, || {})
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
			status.scene_load = None;
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
	use bridge::editor::{
		ViewportInputDto, ViewportModifiersDto, ViewportSceneLoadDto, ViewportSceneLoadStage,
	};
	use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle};
	use renderer::{vulkan::CameraUniform, DrawStatus, WindowRenderer};
	use runtime::{
		assets::AssetManager, scene::Scene, scene_catalog::SceneId, scene_editor::PreparedScene,
		scene_io, Camera,
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

	fn window_handle(window: &Window) -> Result<HWND, String> {
		let handle = window.window_handle().map_err(|error| error.to_string())?;
		let RawWindowHandle::Win32(handle) = handle.as_raw() else {
			return Err("Native viewport window has no Win32 handle".into());
		};
		Ok(handle.hwnd.get())
	}

	fn show_viewport_window(window: &Window) -> Result<(), String> {
		let hwnd = window_handle(window)?;
		window.set_visible(true);
		// Win32 creates child windows below existing siblings. Wry's windowed
		// WebView2 fills the same parent, so showing the viewport is insufficient.
		// SAFETY: The worker owns the live child HWND on this thread.
		let positioned = unsafe {
			SetWindowPos(
				hwnd,
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

	struct KeyboardFocusHook {
		hwnd: HWND,
		_control: Arc<Control>,
	}
	impl KeyboardFocusHook {
		fn new(window: &Window, control: Arc<Control>) -> Result<Self, String> {
			let hwnd = window_handle(window)?;
			// SAFETY: Installed on the child owner thread. The hook retains the
			// control pointer until removal, and is dropped before the window.
			if unsafe {
				SetWindowSubclass(
					hwnd,
					Some(keyboard_focus_proc),
					0,
					Arc::as_ptr(&control) as usize,
				)
			} == 0
			{
				return Err("Could not monitor keyboard focus for the native viewport".into());
			}
			Ok(Self {
				hwnd,
				_control: control,
			})
		}
	}
	impl Drop for KeyboardFocusHook {
		fn drop(&mut self) {
			// SAFETY: The worker drops its own hook on the child owner thread.
			unsafe { RemoveWindowSubclass(self.hwnd, Some(keyboard_focus_proc), 0) };
		}
	}

	unsafe extern "system" fn keyboard_focus_proc(
		hwnd: HWND,
		message: u32,
		wparam: WPARAM,
		lparam: LPARAM,
		id: usize,
		context: usize,
	) -> LRESULT {
		if message == WM_KILLFOCUS {
			// SAFETY: The installed hook owns the Arc backing this reference.
			let control = unsafe { &*(context as *const Control) };
			control.keyboard_blurred.store(true, Ordering::Release);
			control.wake();
		} else if message == WM_NCDESTROY {
			// SAFETY: A destroying HWND can detach its own subclass procedure.
			unsafe { RemoveWindowSubclass(hwnd, Some(keyboard_focus_proc), id) };
		}
		// SAFETY: Forward every message through the existing subclass chain.
		unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
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

	/// Both checks run under the workspace lock, including after GPU preparation.
	/// Obsolete automatic transitions are discarded; an explicit restart must fail visibly.
	enum PlayInstall<'a> {
		Transition {
			generation: u64,
			target: &'a SceneId,
		},
		Restart(&'a ActionRequest),
	}
	impl PlayInstall<'_> {
		fn validate(&self, workspace: &EditorWorkspace, now: Instant) -> Result<bool, String> {
			match self {
				Self::Transition { generation, target } => Ok(workspace.generation == *generation
					&& workspace.backend.pending_play_scene() == Some(*target)),
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
		fn restart_rechecks_deadline_revision_run_and_stop_after_preparation() {
			use bridge::editor::{EditorCommandDto, EditorCommandRequest};
			let assets =
				std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game/assets");
			let mut workspace =
				EditorWorkspace::open(assets.to_str().unwrap(), "scenes/courtyard.json").unwrap();
			let execute = |workspace: &mut EditorWorkspace, command| {
				workspace
					.execute_receipt(EditorCommandRequest {
						expected_revision: workspace.backend.session().revision(),
						command,
					})
					.unwrap()
			};
			execute(&mut workspace, EditorCommandDto::StartPlay);
			let status = workspace.backend.play_status().unwrap();
			let revision = workspace.backend.session().revision();
			let now = Instant::now();
			let mut queued = request(workspace.generation, now + Duration::from_secs(8));
			queued.action = ViewportActionDto::RestartRun {
				run_id: status.run_id.clone(),
				expected_revision: revision,
			};
			let guard = PlayInstall::Restart(&queued);
			assert!(guard.validate(&workspace, now).unwrap());
			let (prepared, _) = workspace
				.backend
				.prepare_restart_play(&status.run_id, revision)
				.unwrap();
			assert!(guard
				.validate(&workspace, now + Duration::from_secs(9))
				.unwrap_err()
				.contains("expired"));
			assert_eq!(
				workspace.backend.play_status().unwrap(),
				status,
				"expired staging leaves old run intact"
			);
			workspace.backend.commit_play_scene(prepared).unwrap();
			assert!(
				guard.validate(&workspace, now).is_err(),
				"queued duplicate cannot restart the new run"
			);
			let fresh = workspace.backend.play_status().unwrap();
			queued.action = ViewportActionDto::RestartRun {
				run_id: fresh.run_id.clone(),
				expected_revision: revision,
			};
			let (prepared, _) = workspace
				.backend
				.prepare_restart_play(&fresh.run_id, revision)
				.unwrap();
			execute(&mut workspace, EditorCommandDto::SetPaused { paused: true });
			assert!(PlayInstall::Restart(&queued)
				.validate(&workspace, now)
				.is_err());
			execute(&mut workspace, EditorCommandDto::StopPlay);
			assert!(PlayInstall::Restart(&queued)
				.validate(&workspace, now)
				.is_err());
			assert!(workspace.backend.commit_play_scene(prepared).is_err());
			assert!(workspace.backend.play_status().is_none());
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

		#[test]
		fn a_new_frame_for_the_same_scene_advances_readiness() {
			let scene_id = runtime::scene_catalog::SceneId::new("level-one").unwrap();
			let other = runtime::scene_catalog::SceneId::new("level-two").unwrap();
			let mut status = ViewportStatusDto {
				scene_load: Some(ViewportSceneLoadDto {
					workspace_generation: 41,
					scene_id: scene_id.clone(),
					scene_revision: 7,
					stage: ViewportSceneLoadStage::Presented,
					completed: None,
					total: None,
				}),
				..ViewportStatusDto::default()
			};
			assert!(!advance_presented_revision(
				&mut status,
				42,
				Some(&scene_id),
				9
			));
			assert!(!advance_presented_revision(
				&mut status,
				41,
				Some(&other),
				9
			));
			assert!(!advance_presented_revision(
				&mut status,
				41,
				Some(&scene_id),
				7
			));
			// A hidden viewport can skip an intervening scene and render this
			// scene again at a new revision. The old presented event is stale.
			assert!(advance_presented_revision(
				&mut status,
				41,
				Some(&scene_id),
				9
			));
			assert_eq!(
				serde_json::to_value(status.scene_load).unwrap(),
				serde_json::json!({
					"workspace_generation": 41,
					"scene_id": "level-one",
					"scene_revision": 9,
					"stage": "presented",
				})
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
		asset_changes: Mutex<VecDeque<crate::asset_reimport_gpu::AssetChangeRequest>>,
		keyboard_blurred: AtomicBool,
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
					status.scene_load = None;
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
				asset_changes: Mutex::new(VecDeque::new()),
				keyboard_blurred: AtomicBool::new(false),
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
		pub(super) fn request_asset_change(
			&self,
			request: crate::asset_reimport_gpu::AssetChangeRequest,
		) -> Result<(), String> {
			if self.stopping() {
				return Err("Viewport is shutting down".into());
			}
			let mut requests = self
				.asset_changes
				.lock()
				.map_err(|_| "Asset publication queue is poisoned")?;
			if self.stopping() {
				return Err("Viewport is shutting down".into());
			}
			if requests.len() >= 4 {
				return Err(
					"Asset publication queue is full; wait for the current operation".into(),
				);
			}
			requests.push_back(request);
			drop(requests);
			self.wake();
			Ok(())
		}
		fn cancel_asset_changes(&self) {
			if let Ok(mut requests) = self.asset_changes.lock() {
				for request in requests.drain(..) {
					let _ = request.reply.send(Err(
						"Native viewport stopped before asset publication".into(),
					));
				}
			}
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
			self.cancel_asset_changes();
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
		assets: ManuallyDrop<AssetManager>,
		renderer: WindowRenderer,
		asset_root: PathBuf,
		camera: Camera,
		revision: u64,
		generation: u64,
		change_cursor: u64,
		assets_invalidated: bool,
	}

	struct RenderPacket {
		document: Option<Arc<runtime::SceneData>>,
		camera: Camera,
		revision: u64,
		generation: u64,
		scene_id: Option<runtime::scene_catalog::SceneId>,
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
				.retire_resources((std::mem::take(&mut self.scene), assets))
				.is_err()
			{
				engine_logger::error!(target: "editor_viewport", "Viewport teardown failed: operation=gpu_wait_idle, generation={}", self.generation);
			}
			engine_logger::debug!(target: "editor_viewport", "Viewport GPU state retired: generation={}", self.generation);
		}
	}

	struct Worker {
		gpu: Option<GpuState>,
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
				gpu: None,
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
		fn update_window_visibility(&mut self) {
			if let Some(window) = self.window.as_ref() {
				if self.active() && !self.failed {
					if self.focus_hook.is_none() {
						match KeyboardFocusHook::new(window, Arc::clone(&self.control)) {
							Ok(hook) => self.focus_hook = Some(hook),
							Err(error) => {
								self.fail("window_keyboard_focus", error);
								return;
							}
						}
					}
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
		fn native_keyboard_focused(&self) -> bool {
			// SAFETY: Read-only queries for the worker's own input queue.
			self.focus_hook.as_ref().is_some_and(|hook| unsafe {
				GetForegroundWindow() == self.hwnd && GetFocus() == hook.hwnd
			})
		}
		fn note_keyboard_focus(&mut self, focused: bool) {
			if !focused {
				self.modifiers = ModifiersState::empty();
				self.pending_motion = None;
				self.pending_fly_motion = [0.0; 2];
			}
			if self.interaction.keyboard_focused() != focused {
				let event = if focused { "focused" } else { "blurred" };
				self.process_input(event, None, None, false, None);
				self.emit_input(event, None, None, false, None);
			}
		}
		fn drain_native_blur(&mut self) {
			if self.control.keyboard_blurred.swap(false, Ordering::AcqRel) {
				self.note_keyboard_focus(false);
			}
		}
		fn focus_keyboard(&mut self) {
			let Some(hook) = self.focus_hook.as_ref() else {
				return;
			};
			// SAFETY: SetFocus targets our child on its owning thread, and only
			// while the editor is the foreground window.
			if self.active() && !self.failed && unsafe { GetForegroundWindow() == self.hwnd } {
				unsafe { SetFocus(hook.hwnd) };
				self.note_keyboard_focus(self.native_keyboard_focused());
			}
		}
		fn release_keyboard_focus(&mut self) {
			let was_focused = self.interaction.keyboard_focused();
			self.note_keyboard_focus(false);
			let Some(hook) = self.focus_hook.as_ref() else {
				return;
			};
			// SAFETY: Clear focus only when our own child still owns it.
			if unsafe { GetFocus() } != hook.hwnd {
				return;
			}
			unsafe { SetFocus(0) };
			if !was_focused {
				// Escape already updated interaction state before releasing the HWND.
				self.emit_input("blurred", None, None, false, None);
			}
			let parent = self.parent.clone();
			let hwnd = self.hwnd;
			let _ = self.parent.run_on_main_thread(move || {
				// An intervening click or dialog owns its own focus. Wry's
				// programmatic WebView focus restores the prior HTML element.
				let (foreground, focused) = unsafe { (GetForegroundWindow(), GetFocus()) };
				if foreground == hwnd && (focused == 0 || focused == hwnd) {
					let _ = parent.as_ref().set_focus();
				}
			});
		}
		fn refresh_game_keyboard_focus(&mut self) {
			let game_active = self.interaction.game_keyboard_active();
			if game_active != self.game_keyboard_active {
				self.game_keyboard_active = game_active;
				self.game_focus_pending = game_active;
				if !game_active {
					self.release_keyboard_focus();
				}
			}
			if self.game_focus_pending && self.active() && !self.failed && self.focus_hook.is_some()
			{
				// This is a one-shot request on entering Play/Game. Escape, blur,
				// and restoring a hidden surface must not reacquire it.
				self.game_focus_pending = false;
				self.focus_keyboard();
			}
		}
		fn flush_fly_motion(&mut self) {
			let delta = std::mem::take(&mut self.pending_fly_motion);
			if delta != [0.0; 2] && self.interaction.is_flying() && self.native_keyboard_focused() {
				self.interaction.look_from_fly_motion(delta);
				self.dirty = true;
			}
		}
		fn clear_play_input(&mut self) {
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					self.interaction.clear_play_input(workspace);
				}
			}
			self.last_tick = None;
		}

		fn tick_play(&mut self, elapsed: f64) -> Result<(), String> {
			let transition = {
				let mut workspace = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let workspace = workspace.as_mut().ok_or("No editor document is open")?;
				if !self.gpu_matches(workspace) {
					self.last_tick = None;
					self.dirty = true;
					return Ok(());
				}
				if !workspace.backend.has_play_visibility_capabilities() {
					let gpu = self.gpu.as_ref().expect("matching GPU scene checked above");
					if gpu.revision != workspace.backend.session().scene_revision()
						|| gpu.assets_invalidated
					{
						self.last_tick = None;
						self.dirty = true;
						return Ok(());
					}
					workspace
						.backend
						.set_play_visibility_capabilities(gpu.scene.visibility_capabilities())?;
				}
				let outcome = workspace.backend.tick(elapsed);
				self.interaction
					.sync_game_status_after_tick(workspace, &outcome);
				let outcome = outcome.map_err(|error| error.to_string())?;
				self.dirty |= !outcome.changes.is_empty();
				outcome.requested_scene.map(|target| {
					(
						workspace.generation,
						target,
						workspace.backend.prepare_pending_play_scene(),
					)
				})
			};

			if let Some((generation, target, prepared)) = transition {
				self.last_tick = None;
				let result = prepared
					.map_err(|error| error.to_string())
					.and_then(|prepared| {
						self.install_play_scene(
							PlayInstall::Transition {
								generation,
								target: &target,
							},
							prepared,
						)
					});
				if let Err(error) = result {
					let current = self.shared.lock().ok().is_some_and(|workspace| {
						workspace.as_ref().is_some_and(|workspace| {
							workspace.generation == generation
								&& workspace.backend.pending_play_scene() == Some(&target)
						})
					});
					if current {
						engine_logger::error!(target: "editor_viewport", "Play scene transition failed: operation=install_play_scene");
						self.interaction.report(format!(
							"Could not switch to game scene {:?}: {error}",
							target.as_str()
						));
					}
				}
			}
			Ok(())
		}

		fn gpu_matches(&self, workspace: &EditorWorkspace) -> bool {
			self.gpu.as_ref().is_some_and(|gpu| {
				gpu.generation == workspace.generation && gpu.asset_root == workspace.asset_root
			})
		}

		fn install_play_scene(
			&mut self,
			guard: PlayInstall<'_>,
			(prepared, document): (PreparedScene, Arc<runtime::SceneData>),
		) -> Result<(), String> {
			let gpu = self
				.gpu
				.as_mut()
				.ok_or("Native viewport renderer is not initialized")?;
			let candidate = scene_io::instantiate(&document, &mut gpu.assets, &gpu.renderer);
			drop(document);
			gpu.renderer.wait_idle()?;
			let installed = candidate.and_then(|(scene, camera)| {
				let mut shared = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let Some(workspace) = shared.as_mut() else {
					return match guard {
						PlayInstall::Transition { .. } => Ok(()),
						PlayInstall::Restart(_) => {
							Err("The project was closed during restart".into())
						}
					};
				};
				if !guard.validate(workspace, Instant::now())? {
					return Ok(());
				}

				// Lights are validated CPU data. Keep renderer uploads and waits
				// outside the workspace lock and restore the old lights on rejection.
				gpu.renderer.set_lights(&scene.renderer_lights())?;
				if let Err(error) = workspace.backend.commit_play_scene(prepared) {
					gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
					return Err(error.to_string());
				}
				let retired_scene = std::mem::replace(&mut gpu.scene, scene);
				gpu.camera = camera;
				gpu.revision = workspace.backend.session().scene_revision();
				gpu.change_cursor = workspace.backend.change_cursor();
				self.interaction.clear_play_input(workspace);
				self.interaction.clear_error();
				self.interaction.sync_game_status(workspace);
				self.last_tick = None;
				self.dirty = true;
				drop(shared);
				drop(retired_scene);
				Ok(())
			});
			// Candidate-only resources are also pruned after a stale or failed commit.
			gpu.assets.prune_unused();
			installed
		}

		fn restart_run(&mut self, request: &ActionRequest) -> Result<(), String> {
			let prepared = {
				let shared = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let workspace = shared
					.as_ref()
					.ok_or("Open a project before restarting a run")?;
				PlayInstall::Restart(request).validate(workspace, Instant::now())?;
				if !self.gpu_matches(workspace) {
					return Err(
						"Wait for the current project to finish loading before restarting".into(),
					);
				}
				let ViewportActionDto::RestartRun {
					run_id,
					expected_revision,
				} = &request.action
				else {
					return Err("Expected a Restart Run request".into());
				};
				workspace
					.backend
					.prepare_restart_play(run_id, *expected_revision)
					.map_err(|error| error.to_string())?
			};
			self.install_play_scene(PlayInstall::Restart(request), prepared)
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
						"Viewport is synchronizing the current project; retry asset publication"
							.into(),
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
		fn process_input(
			&mut self,
			kind: &str,
			button: Option<String>,
			code: Option<String>,
			repeat: bool,
			delta: Option<[f64; 2]>,
		) {
			let extent = self.prepared_extent.unwrap_or_else(|| self.extent());
			if extent.contains(&0) && !matches!(kind, "focused" | "blurred") {
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
			let before = self.interaction.revision();
			if let Ok(mut workspace) = self.shared.lock() {
				if let Some(workspace) = workspace.as_mut() {
					if let Err(error) = self.interaction.input(&input, workspace, extent, self.dpi)
					{
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
				self.release_keyboard_focus();
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
				self.update_capture();
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
								&& !gpu.assets_invalidated
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
											runtime::events::BackendEventKind::RuntimeSceneChanged { .. } => {
												changes.full_resync = true;
											}
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
						let camera = session.render_camera();
						Some(RenderPacket {
							document,
							camera,
							revision,
							generation,
							scene_id: (!session.is_playing()
								&& self.interaction.settings.view
									== bridge::viewport::ViewportView::Scene)
								.then(|| session.active_scene_id().clone()),
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
				scene_id,
				change_cursor,
				asset_root,
				lights_changed,
			} = packet;
			let previous = scene_id.as_ref().and_then(|scene_id| {
				self.status.lock().ok().and_then(|status| {
					status.scene_load.as_ref().and_then(|progress| {
						(progress.workspace_generation == generation
							&& &progress.scene_id == scene_id)
							.then_some(progress.stage)
					})
				})
			});
			let loading = scene_id.is_some() && previous != Some(ViewportSceneLoadStage::Presented);
			let (parent, control, status) = (&self.parent, &self.control, &self.status);
			let report = |stage, count: Option<(usize, usize)>| {
				if let Some(scene_id) = scene_id.as_ref() {
					let (completed, total) = count.unzip();
					publish_scene_load(
						parent,
						control,
						status,
						ViewportSceneLoadDto {
							workspace_generation: generation,
							scene_id: scene_id.clone(),
							scene_revision: revision,
							stage,
							completed,
							total,
						},
					);
				}
			};
			if loading && previous.is_none() {
				report(ViewportSceneLoadStage::Preparing, None);
			}
			let mut last_count = None;
			let mut count_prepared = |completed, total| {
				if !loading {
					return;
				}
				let now = Instant::now();
				if completed == total
					|| last_count.is_none_or(|last| now.duration_since(last) >= SCENE_LOAD_INTERVAL)
				{
					report(ViewportSceneLoadStage::Preparing, Some((completed, total)));
					last_count = Some(now);
				}
			};
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
				let (scene, initial_camera) = scene_io::instantiate_with_progress(
					&data,
					&mut assets,
					&renderer,
					&mut count_prepared,
				)?;
				renderer.set_lights(&scene.renderer_lights())?;
				engine_logger::debug!(target: "editor_viewport", "Viewport GPU state created: generation={generation}, width={width}, height={height}, entities={}, lights={}", data.entities.len(), data.lights.len());
				drop(data);
				self.gpu = Some(GpuState {
					scene,
					assets: ManuallyDrop::new(assets),
					renderer,
					asset_root,
					camera: initial_camera,
					revision,
					generation,
					change_cursor,
					assets_invalidated: false,
				});
			}
			let gpu = self
				.gpu
				.as_mut()
				.ok_or("Viewport GPU state is not initialized")?;
			let generation_changed = gpu.generation != generation;
			if gpu.revision != revision || generation_changed || gpu.assets_invalidated {
				let data = document
					.take()
					.ok_or("Full viewport synchronization packet is missing scene data")?;
				let replacement = if gpu.assets_invalidated || !gpu.scene.synchronize(&data)? {
					let (scene, _) = match scene_io::instantiate_with_progress(
						&data,
						&mut gpu.assets,
						&gpu.renderer,
						&mut count_prepared,
					) {
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
				gpu.assets_invalidated = false;
			} else if lights_changed {
				gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
			}
			gpu.camera = camera;
			// Do not hold a strong document snapshot across frame fences or
			// presentation. Play can mutate its uniquely owned Arc in place.
			drop(document);
			if loading {
				report(ViewportSceneLoadStage::Rendering, None);
			}
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
			let frame_status = gpu.renderer.draw(&uniform, &objects)?;
			match frame_status {
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
					if !loading && frame_status == DrawStatus::Rendered {
						self.status_pending |= advance_presented_revision(
							&mut status,
							generation,
							scene_id.as_ref(),
							revision,
						);
					}
				}
			}
			if loading && frame_status == DrawStatus::Rendered {
				report(ViewportSceneLoadStage::Presented, None);
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
				if self.interaction.is_flying()
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
					if !self.interaction.is_flying() {
						self.pending_motion = Some(self.cursor);
					}
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
					self.flush_fly_motion();
					if state == ElementState::Pressed
						&& matches!(button.as_str(), "left" | "middle" | "right")
					{
						self.focus_keyboard();
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
				WindowEvent::KeyboardInput {
					event,
					is_synthetic: false,
					..
				} if self.interaction.keyboard_focused() && self.native_keyboard_focused() => {
					self.flush_fly_motion();
					let release = event.state == ElementState::Pressed
						&& !event.repeat && event.physical_key
						== PhysicalKey::Code(KeyCode::Escape);
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
			self.refresh_game_keyboard_focus();
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
}
