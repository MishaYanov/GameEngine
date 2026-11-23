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

	pub fn refresh_sky(&self, generation: u64) {
		#[cfg(windows)]
		if let Ok(running) = self.running.lock() {
			if let Some(running) = running.as_ref() {
				running.control.refresh_sky(generation);
			}
		}
		#[cfg(not(windows))]
		let _ = generation;
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
			let control = if matches!(
				change.asset().kind,
				runtime::asset_catalog::AssetKind::AudioClip
					| runtime::asset_catalog::AssetKind::SkyPanorama
			) {
				None
			} else {
				self.reap();
				self.running
					.lock()
					.map_err(|_| "Viewport lifecycle lock is poisoned")?
					.as_ref()
					.map(|running| Arc::clone(&running.control))
			};
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
		// Audio Clip publication uses only the catalog. If the viewport is
		// detached, other asset kinds also have no GPU caches to replace.
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
mod windows;
