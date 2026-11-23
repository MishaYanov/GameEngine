//! Preview rendering and navigation on the existing child window and device.
use super::*;
use crate::{
	editor::prefab_sources::PropDraftEdit,
	editor::prop_preview::PropPreviewFrame,
	viewport_tools::{adaptive_grid_spacing, pick_nearest, EditorCamera, PickTarget},
};
use bridge::viewport::ViewportProjection;
use glam::Vec2;
use renderer::{vulkan::Light, EditorGrid, EditorVisuals};
use runtime::{scene::EntityId, Bounds3, EntityKey};

#[path = "prop_preview/translate.rs"]
mod translate;
use translate::{move_handles, PropTranslationDrag};

pub(super) struct PropGpuPreview {
	token: String,
	content_revision: u64,
	scene: Scene,
	targets: Vec<PickTarget>,
	bounds: Option<Bounds3>,
	temporary_position: Option<(EntityId, [f32; 3])>,
	pose_revision: Option<u64>,
	visual_revision: Option<u64>,
}

impl PropGpuPreview {
	fn new(token: String, content_revision: u64, scene: Scene) -> Result<Self, String> {
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
			content_revision,
			scene,
			targets,
			bounds,
			temporary_position: None,
			pose_revision: None,
			visual_revision: None,
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

	fn preview_position(&mut self, drag: Option<&PropTranslationDrag>) {
		if let Some((id, baseline)) = self.temporary_position.take() {
			if let Some(entity) = self.scene.get_mut(id) {
				entity.transform_mut().position = baseline;
			}
		}
		if let Some((id, position)) = drag.filter(|drag| drag.moved()).and_then(|drag| {
			self.scene
				.entity_by_key(&drag.key)
				.map(|id| (id, drag.position()))
		}) {
			if let Some(entity) = self.scene.get_mut(id) {
				let local = entity.transform_mut();
				self.temporary_position = Some((id, local.position));
				local.position = position;
			}
		}
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
	translation: Option<PropTranslationDrag>,
	frame_request: Option<u64>,
}

impl PropNavigation {
	fn new(token: String) -> Self {
		Self {
			token,
			camera: EditorCamera::default(),
			drag: None,
			translation: None,
			frame_request: None,
		}
	}

	pub(super) fn captured(&self) -> bool {
		self.drag.is_some() || self.translation.is_some()
	}

	pub(super) fn cancel(&mut self) {
		self.drag = None;
		self.translation = None;
	}

	fn retain_translation(&mut self, frame: &PropPreviewFrame, extent: [u32; 2]) -> bool {
		if self.translation.as_ref().is_some_and(|drag| {
			drag.revision != frame.content_revision
				|| drag.extent != extent
				|| frame.state.selection.as_ref() != Some(&drag.key)
		}) {
			self.translation = None;
			true
		} else {
			false
		}
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
				slot.as_mut().is_some_and(|workspace| {
					workspace
						.prop_preview_content_is_current(&frame.state.token, frame.content_revision)
						&& workspace.close_prop_preview(&frame.state.token)
				})
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
				slot.as_ref().is_some_and(|workspace| {
					workspace.prop_preview_content_is_current(token, frame.content_revision)
				})
			})
		};
		let gpu = self
			.gpu
			.as_mut()
			.ok_or("Viewport renderer is not initialized")?;
		if gpu.prop_preview.as_ref().is_none_or(|preview| {
			&preview.token != token || preview.content_revision != frame.content_revision
		}) {
			if let Some(preview) = gpu.prop_preview.take() {
				gpu.renderer.retire_resources(preview)?;
				gpu.assets.prune_unused();
			}
			if let Some((_, pose)) = &frame.pose {
				// Seed the existing loader with the reviewed bytes already held by the preview.
				gpu.assets
					.load_animation(&gpu.renderer, Arc::clone(&pose.asset))
					.map_err(|error| error.to_string())?;
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
			let preview = PropGpuPreview::new(token.clone(), frame.content_revision, scene)?;
			gpu.renderer.set_lights(&[Light::Directional {
				direction: [0.2, -0.9, -0.4],
				color: [1.0; 3],
				intensity: 0.85,
			}])?;
			gpu.prop_preview = Some(preview);
		}
		let preview = gpu
			.prop_preview
			.as_mut()
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
		let release_translation = navigation.retain_translation(frame, extent);
		preview.preview_position(navigation.translation.as_ref());
		if let Some(visual) = &frame.visual_pose {
			if preview.visual_revision != Some(visual.revision) {
				for pose in visual.poses.iter() {
					let id = preview
						.scene
						.entity_by_key(&pose.entity_key)
						.ok_or("Animation Clip object is missing from the viewport")?;
					let entity = preview
						.scene
						.get_mut(id)
						.ok_or("Animation Clip object is missing from the viewport")?;
					let target = entity.transform_mut();
					let local = pose.transform();
					target.position = local.position;
					target.rotation = local.rotation;
				}
				preview.visual_revision = Some(visual.revision);
			}
		}
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
			lines: if frame.editable_source {
				move_handles(
					&preview.scene,
					&navigation.camera,
					extent,
					self.dpi,
					frame.state.selection.as_ref(),
				)
			} else {
				Vec::new()
			},
		})?;
		let frustum = runtime::visibility::Frustum::from_view_projection(
			glam::Mat4::from_cols_array(&uniform.view_projection),
		)?;
		if let Some((key, pose)) = &frame.pose {
			if preview.pose_revision != Some(pose.revision) {
				preview.scene.set_animation_pose(key, Some(pose))?;
				preview.pose_revision = Some(pose.revision);
			}
		}
		let (objects, _) = preview.scene.try_render_visible(Some(&frustum))?;
		if !current() {
			self.dirty = true;
			return Ok(());
		}
		gpu.renderer.set_sky(None);
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
		if release_translation {
			self.update_capture();
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
			.filter(|preview| {
				&preview.token == token && preview.content_revision == frame.content_revision
			});
		let Some(navigation) = self
			.prop_navigation
			.as_mut()
			.filter(|navigation| &navigation.token == token)
		else {
			return true;
		};
		navigation.retain_translation(&frame, extent);
		match (kind, button, code, point) {
			("pointer_down", Some("right"), _, Some(point)) if !navigation.captured() => {
				navigation.drag = Some((DragKind::Orbit, point));
			}
			("pointer_down", Some("middle"), _, Some(point)) if !navigation.captured() => {
				navigation.drag = Some((DragKind::Pan, point));
			}
			("pointer_down", Some("left"), _, Some(point))
				if self.modifiers.alt_key() && !navigation.captured() =>
			{
				navigation.drag = Some((DragKind::Orbit, point));
			}
			("pointer_down", Some("left"), _, Some(point)) if !navigation.captured() => {
				if frame.editable_source {
					navigation.translation = ready.and_then(|preview| {
						PropTranslationDrag::begin(
							&preview.scene,
							&navigation.camera,
							extent,
							self.dpi,
							frame.state.selection.clone()?,
							frame.content_revision,
							point,
						)
					});
				}
				if let Some((preview, ray)) = ready
					.filter(|_| navigation.translation.is_none())
					.and_then(|preview| {
						navigation
							.camera
							.ray(point, extent, ViewportProjection::Perspective)
							.ok()
							.map(|ray| (preview, ray))
					}) {
					let key = pick_nearest(ray, &preview.targets)
						.and_then(|hit| EntityKey::new(hit.key).ok());
					let accepted = self.shared.lock().ok().and_then(|mut slot| {
						let workspace = slot.as_mut()?;
						if workspace.prop_preview_content_is_current(token, frame.content_revision)
						{
							workspace.select_prop_preview(token, key).ok()
						} else {
							None
						}
					});
					if let Some(state) = accepted {
						let _ = self.parent.emit("editor://prop-preview-selection", state);
						self.dirty = true;
					}
				}
			}
			("pointer_move", _, _, Some(point)) => {
				if let Some(drag) = navigation.translation.as_mut() {
					if drag.update(point).is_err() {
						navigation.translation = None;
					}
					self.dirty = true;
				} else if let Some((drag, previous)) = navigation.drag.as_mut() {
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
			("pointer_up", Some("left"), _, _) if navigation.translation.is_some() => {
				let drag = navigation
					.translation
					.take()
					.expect("checked Prop translation");
				self.dirty = true;
				if drag.moved() {
					let updated = self.shared.lock().ok().and_then(|mut slot| {
						slot.as_mut()?
							.apply_prop_preview_position(
								token,
								drag.revision,
								&drag.key,
								drag.position(),
							)
							.ok()
					});
					if let Some(updated) = updated {
						let _ = self.parent.emit("editor://prop-draft-changed", &updated);
					}
				}
			}
			("pointer_up" | "pointer_leave" | "blurred", _, _, _)
			| ("key_down", _, Some("Escape"), _) => {
				self.dirty |= navigation.captured();
				navigation.cancel();
			}
			("wheel_lines" | "wheel_pixels", _, _, Some(_)) if !navigation.captured() => {
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
			("key_down", _, Some("KeyF"), _) if !repeat && !navigation.captured() => {
				self.dirty |= self.shared.lock().is_ok_and(|mut slot| {
					slot.as_mut()
						.is_some_and(|workspace| workspace.frame_prop_preview(token).is_ok())
				});
			}
			("key_down", _, Some("KeyZ" | "KeyY"), _)
				if !repeat && self.modifiers.control_key() && frame.editable_source =>
			{
				navigation.cancel();
				self.dirty = true;
				let undo = code == Some("KeyZ") && !self.modifiers.shift_key();
				let edit = if undo {
					PropDraftEdit::Undo
				} else {
					PropDraftEdit::Redo
				};
				let updated = self.shared.lock().ok().and_then(|mut slot| {
					let workspace = slot.as_mut()?;
					let id = workspace
						.prop_preview_edit_draft(token, frame.content_revision)
						.ok()?;
					workspace
						.edit_prop_draft(&id, frame.content_revision, edit)
						.ok()
				});
				if let Some(updated) = updated {
					let _ = self.parent.emit("editor://prop-draft-changed", &updated);
				}
			}
			_ => {}
		}
		self.update_capture();
		true
	}
}
