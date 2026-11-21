use super::*;

pub(super) struct Latest {
	pub(super) bounds: ViewportBoundsDto,
	pub(super) dpi: f64,
	pub(super) invalidated: bool,
	pub(super) attached: bool,
	pub(super) parent_visible: bool,
	pub(super) clear_input: bool,
	pub(super) stop: bool,
	pub(super) wake_queued: bool,
}

pub(in crate::viewport) struct Control {
	pub(super) latest: Mutex<Latest>,
	pub(super) proxy: Mutex<Option<EventLoopProxy<Wake>>>,
	pub(super) actions: Mutex<VecDeque<ActionRequest>>,
	pub(super) asset_changes: Mutex<VecDeque<crate::asset_reimport_gpu::AssetChangeRequest>>,
	pub(super) keyboard_blurred: AtomicBool,
}
impl Control {
	pub(in crate::viewport) fn requested_status(
		&self,
		mut status: ViewportStatusDto,
	) -> ViewportStatusDto {
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
	pub(in crate::viewport) fn new(bounds: ViewportBoundsDto, dpi: f64) -> Result<Self, String> {
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
	pub(in crate::viewport) fn request_action(
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
	pub(in crate::viewport) fn request_asset_change(
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
			return Err("Asset publication queue is full; wait for the current operation".into());
		}
		requests.push_back(request);
		drop(requests);
		self.wake();
		Ok(())
	}
	pub(super) fn cancel_asset_changes(&self) {
		if let Ok(mut requests) = self.asset_changes.lock() {
			for request in requests.drain(..) {
				let _ = request.reply.send(Err(
					"Native viewport stopped before asset publication".into()
				));
			}
		}
	}
	pub(super) fn validate_bounds(bounds: ViewportBoundsDto, dpi: f64) -> Result<(), String> {
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
	pub(in crate::viewport) fn attach(
		&self,
		bounds: ViewportBoundsDto,
		dpi: f64,
	) -> Result<(), String> {
		self.set_bounds(bounds, dpi)?;
		if let Ok(mut latest) = self.latest.lock() {
			latest.attached = true;
			latest.invalidated = true;
		}
		self.wake();
		Ok(())
	}
	pub(in crate::viewport) fn detach(&self) {
		if let Ok(mut latest) = self.latest.lock() {
			latest.attached = false;
			latest.invalidated = true;
			latest.clear_input = true;
		}
		self.wake();
	}
	pub(in crate::viewport) fn set_parent_visible(&self, visible: bool) {
		if let Ok(mut latest) = self.latest.lock() {
			latest.parent_visible = visible;
			latest.invalidated = true;
			latest.clear_input |= !visible;
		}
		self.wake();
	}
	pub(super) fn wake(&self) {
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
	pub(in crate::viewport) fn set_bounds(
		&self,
		bounds: ViewportBoundsDto,
		dpi: f64,
	) -> Result<(), String> {
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
	pub(in crate::viewport) fn invalidate(&self) {
		if let Ok(mut latest) = self.latest.lock() {
			latest.invalidated = true;
		}
		self.wake();
	}
	pub(in crate::viewport) fn stop(&self) {
		if let Ok(mut latest) = self.latest.lock() {
			latest.stop = true;
		}
		self.cancel_asset_changes();
		self.wake();
	}
	pub(in crate::viewport) fn stopping(&self) -> bool {
		self.latest.lock().map(|latest| latest.stop).unwrap_or(true)
	}
}
