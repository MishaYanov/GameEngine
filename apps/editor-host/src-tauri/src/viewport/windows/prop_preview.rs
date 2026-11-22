//! Preview rendering and navigation on the existing child window and device.
use super::*;
use crate::{
	editor::prop_preview::PropPreviewFrame,
	viewport_tools::{adaptive_grid_spacing, pick_nearest, EditorCamera, PickTarget},
};
use bridge::viewport::ViewportProjection;
use glam::Vec2;
use renderer::{vulkan::Light, EditorGrid, EditorVisuals};
use runtime::{Bounds3, EntityKey};

pub(super) struct PropGpuPreview {
	token: String,
	scene: Scene,
	targets: Vec<PickTarget>,
	bounds: Option<Bounds3>,
}

impl PropGpuPreview {
	fn new(token: String, scene: Scene) -> Result<Self, String> {
		let mut targets = Vec::new();
		for id in scene.entity_ids() {
			if let (Some(key), Some(bounds)) = (scene.key(id), scene.world_bounds(id)?) {
				targets.push(PickTarget {
					key: key.as_str().into(),
					bounds,
				});
			}
		}
		let bounds = targets
			.iter()
			.map(|target| target.bounds)
			.reduce(Bounds3::union);
		Ok(Self {
			token,
			scene,
			targets,
			bounds,
		})
	}

	fn frame_bounds(&self, selection: Option<&EntityKey>) -> Option<Bounds3> {
		selection
			.and_then(|selection| {
				self.targets
					.iter()
					.find(|target| target.key == selection.as_str())
			})
			.map(|target| target.bounds)
			.or(self.bounds)
	}
}

#[derive(Clone, Copy)]
enum DragKind {
	Orbit,
	Pan,
}

pub(super) struct PropNavigation {
	token: String,
	camera: EditorCamera,
	drag: Option<(DragKind, [f64; 2])>,
	frame_request: Option<u64>,
}

impl PropNavigation {
	fn new(token: String) -> Self {
		Self {
			token,
			camera: EditorCamera::default(),
			drag: None,
			frame_request: None,
		}
	}

	pub(super) fn captured(&self) -> bool {
		self.drag.is_some()
	}

	pub(super) fn cancel(&mut self) {
		self.drag = None;
	}
}

impl Worker {
	pub(super) fn try_draw_prop_preview(&mut self) -> Result<bool, String> {
		let current = self.shared.lock().ok().and_then(|slot| {
			let workspace = slot.as_ref()?;
			workspace
				.active_prop_preview()
				.map(|frame| (frame, workspace.asset_root.clone()))
		});
		let Some((frame, asset_root)) = current else {
			let returning_to_scene = self.prop_navigation.take().is_some();
			if returning_to_scene {
				self.release_keyboard_focus();
				self.update_capture();
			}
			if let Some(gpu) = self.gpu.as_mut() {
				if let Some(preview) = gpu.prop_preview.take() {
					gpu.renderer.retire_resources(preview)?;
					gpu.assets.prune_unused();
				}
				if returning_to_scene {
					gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
				}
			}
			return Ok(false);
		};
		if self
			.prop_navigation
			.as_ref()
			.is_none_or(|navigation| navigation.token != frame.state.token)
		{
			self.release_keyboard_focus();
			self.cancel_editor();
			self.pending_fly_motion = [0.0; 2];
			self.prop_navigation = Some(PropNavigation::new(frame.state.token.clone()));
			self.update_capture();
		}
		self.authored_load = None;
		self.play_load = None;
		if let Err(message) = self.draw_prop_preview(&frame, asset_root) {
			let closed = self.shared.lock().ok().is_some_and(|mut slot| {
				slot.as_mut()
					.is_some_and(|workspace| workspace.close_prop_preview(&frame.state.token))
			});
			if closed {
				let state = &frame.state;
				let _ = self.parent.emit(
					"editor://prop-preview-error",
					serde_json::json!({
						"token": state.token,
						"workspace_generation": state.workspace_generation,
						"revision": state.revision,
						"request_id": state.request_id,
						"selection": state.selection,
						"message": message,
					}),
				);
			}
			self.dirty = true;
		}
		Ok(true)
	}

	fn draw_prop_preview(
		&mut self,
		frame: &PropPreviewFrame,
		asset_root: PathBuf,
	) -> Result<(), String> {
		if self
			.gpu
			.as_ref()
			.is_some_and(|gpu| gpu.asset_root != asset_root)
		{
			self.gpu = None;
			self.prepared_extent = None;
		}
		if self.gpu.is_none() {
			let window = Arc::clone(
				self.window
					.as_ref()
					.ok_or("Native viewport window is not initialized")?,
			);
			let [width, height] = self.extent();
			let renderer = WindowRenderer::new(window, width, height)?;
			let assets = AssetManager::new(&asset_root).map_err(|error| error.to_string())?;
			// The first frame can be the Prop pane. Keep an empty deferred
			// authored slot until the same renderer first enters Scene or Game.
			self.gpu = Some(GpuState {
				scene: Scene::default(),
				prop_preview: None,
				assets: ManuallyDrop::new(assets),
				renderer,
				asset_root,
				revision: 0,
				generation: 0,
				change_cursor: 0,
				assets_invalidated: true,
			});
		}
		let token = &frame.state.token;
		let current = || {
			self.shared.lock().is_ok_and(|slot| {
				slot.as_ref()
					.is_some_and(|workspace| workspace.prop_preview_is_current(token))
			})
		};
		let gpu = self
			.gpu
			.as_mut()
			.ok_or("Viewport renderer is not initialized")?;
		if gpu
			.prop_preview
			.as_ref()
			.is_none_or(|preview| &preview.token != token)
		{
			if let Some(preview) = gpu.prop_preview.take() {
				gpu.renderer.retire_resources(preview)?;
				gpu.assets.prune_unused();
			}
			let candidate = match scene_io::instantiate_cancellable(
				&frame.document,
				&mut gpu.assets,
				&gpu.renderer,
				|_, _| {},
				|| !current(),
			) {
				Ok(candidate) => candidate,
				Err(error) => {
					gpu.renderer.wait_idle()?;
					gpu.assets.prune_unused();
					return Err(error);
				}
			};
			let Some((scene, _)) = candidate else {
				gpu.renderer.wait_idle()?;
				gpu.assets.prune_unused();
				self.dirty = true;
				return Ok(());
			};
			if !current() {
				gpu.renderer.retire_resources(scene)?;
				gpu.assets.prune_unused();
				self.dirty = true;
				return Ok(());
			}
			let preview = PropGpuPreview::new(token.clone(), scene)?;
			gpu.renderer.set_lights(&[Light::Directional {
				direction: [0.2, -0.9, -0.4],
				color: [1.0; 3],
				intensity: 0.85,
			}])?;
			gpu.prop_preview = Some(preview);
		}
		let preview = gpu
			.prop_preview
			.as_ref()
			.ok_or("Prop renderer lost its retained preview")?;
		let navigation = self
			.prop_navigation
			.as_mut()
			.ok_or("Prop camera is not initialized")?;
		let Some([width, height]) = gpu.renderer.prepare()? else {
			self.prepared_extent = None;
			return Ok(());
		};
		let extent = [width, height];
		if self.prepared_extent != Some(extent) {
			self.last_status = Instant::now() - STATUS_INTERVAL;
		}
		self.prepared_extent = Some(extent);
		if navigation.frame_request != Some(frame.frame_request) {
			if let Some(bounds) = preview.frame_bounds(frame.state.selection.as_ref()) {
				navigation
					.camera
					.frame(bounds, width as f32 / height as f32);
			}
			navigation.frame_request = Some(frame.frame_request);
		}
		let projection = ViewportProjection::Perspective;
		let uniform = CameraUniform::from_view_projection(
			navigation.camera.view_projection(extent, projection)?,
		);
		let spacing = adaptive_grid_spacing(
			1.0,
			navigation
				.camera
				.units_per_pixel(navigation.camera.pivot, extent, projection),
		);
		gpu.renderer.set_editor_visuals(&EditorVisuals {
			grid: Some(EditorGrid {
				spacing,
				..EditorGrid::default()
			}),
			lines: vec![],
		})?;
		let frustum = runtime::visibility::Frustum::from_view_projection(
			glam::Mat4::from_cols_array(&uniform.view_projection),
		)?;
		let (objects, _) = preview.scene.try_render_visible(Some(&frustum))?;
		if !current() {
			self.dirty = true;
			return Ok(());
		}
		let status = gpu.renderer.draw(&uniform, &objects)?;
		if status == DrawStatus::NeedsRedraw {
			self.dirty = true;
			self.redraw_retry = Some(Instant::now() + FRAME);
		} else {
			self.redraw_retry = None;
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

	pub(super) fn process_prop_input(
		&mut self,
		kind: &str,
		button: Option<&str>,
		code: Option<&str>,
		repeat: bool,
		delta: Option<[f64; 2]>,
	) -> bool {
		let frame = self
			.shared
			.lock()
			.ok()
			.and_then(|slot| slot.as_ref()?.active_prop_preview());
		let Some(frame) = frame else {
			// Consume pointer release/focus events from the retiring aperture.
			return self.prop_navigation.is_some();
		};
		let token = &frame.state.token;
		let (point, extent) = self.raw_input_surface();
		let ready = self
			.gpu
			.as_ref()
			.and_then(|gpu| gpu.prop_preview.as_ref())
			.filter(|preview| &preview.token == token);
		let Some(navigation) = self
			.prop_navigation
			.as_mut()
			.filter(|navigation| &navigation.token == token)
		else {
			return true;
		};
		match (kind, button, code, point) {
			("pointer_down", Some("right"), _, Some(point)) => {
				navigation.drag = Some((DragKind::Orbit, point));
			}
			("pointer_down", Some("middle"), _, Some(point)) => {
				navigation.drag = Some((DragKind::Pan, point));
			}
			("pointer_down", Some("left"), _, Some(point)) if self.modifiers.alt_key() => {
				navigation.drag = Some((DragKind::Orbit, point));
			}
			("pointer_down", Some("left"), _, Some(point)) if !navigation.captured() => {
				if let Some((preview, ray)) = ready.and_then(|preview| {
					navigation
						.camera
						.ray(point, extent, ViewportProjection::Perspective)
						.ok()
						.map(|ray| (preview, ray))
				}) {
					let key = pick_nearest(ray, &preview.targets)
						.and_then(|hit| EntityKey::new(hit.key).ok());
					let accepted =
						self.shared.lock().ok().and_then(|mut slot| {
							slot.as_mut()?.select_prop_preview(token, key).ok()
						});
					if let Some(state) = accepted {
						let _ = self.parent.emit("editor://prop-preview-selection", state);
						self.dirty = true;
					}
				}
			}
			("pointer_move", _, _, Some(point)) => {
				if let Some((drag, previous)) = navigation.drag.as_mut() {
					let motion = Vec2::new(
						(point[0] - previous[0]) as f32,
						(point[1] - previous[1]) as f32,
					);
					*previous = point;
					match drag {
						DragKind::Orbit => navigation.camera.orbit(motion / self.dpi as f32),
						DragKind::Pan => {
							navigation
								.camera
								.pan(motion, extent, ViewportProjection::Perspective)
						}
					}
					self.dirty = true;
				}
			}
			("pointer_up" | "pointer_leave" | "blurred", _, _, _)
			| ("key_down", _, Some("Escape"), _) => navigation.cancel(),
			("wheel_lines" | "wheel_pixels", _, _, Some(_)) => {
				if let Some([_, distance]) = delta {
					let lines = if kind == "wheel_pixels" {
						distance / (self.dpi * 32.0)
					} else {
						distance
					};
					navigation
						.camera
						.zoom((-0.12 * lines.clamp(-12.0, 12.0) as f32).exp());
					self.dirty = true;
				}
			}
			("key_down", _, Some("KeyF"), _) if !repeat => {
				self.dirty |= self.shared.lock().is_ok_and(|mut slot| {
					slot.as_mut()
						.is_some_and(|workspace| workspace.frame_prop_preview(token).is_ok())
				});
			}
			_ => {}
		}
		self.update_capture();
		true
	}
}
