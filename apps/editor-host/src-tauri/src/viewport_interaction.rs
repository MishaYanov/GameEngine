//! Native editor interaction ownership, independent of the window and DOM.
mod authoring;
#[path = "collider_visuals.rs"]
mod collider_visuals;
mod game_input;
mod navigation;
mod overlays;

pub(crate) use game_input::GameCameraCapture;

use std::{
	cell::{Ref, RefCell},
	collections::{HashMap, HashSet},
};

use bridge::editor::{
	EditorCommandDto as Command, EditorCommandRequest, TransformDto, TransformEditDto,
	TransformSpace, ViewportInputDto,
};
use bridge::viewport::*;
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use renderer::{EditorGrid, EditorLine, EditorVisuals};
use runtime::camera_input::{CameraInputOwner, CameraIntent};
use runtime::{scene::Scene, simulation_session::TickOutcome, BackendError, Bounds3};

use crate::editor::{AssetTicket, EditorWorkspace};
use crate::viewport_tools::{
	self as tools, EditorCamera, Gizmo, GizmoDrag, PickTarget, TransformTarget,
};

const TRANSFORM_LABEL: &str = "Viewport transform";
const MAX_VIEW_BOOKMARKS: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavigationMode {
	Orbit,
	Pan,
	Dolly,
	Fly,
}
struct Navigation {
	mode: NavigationMode,
	previous: Vec2,
	button: Option<&'static str>,
	walk_origin: Option<EditorCamera>,
}
struct Click {
	start: [f64; 2],
	current: [f64; 2],
	selection: Vec<String>,
	additive: bool,
	toggle: bool,
}
struct Placement {
	path: String,
	normalized: Option<[f64; 2]>,
	position: Option<Vec3>,
}
#[derive(Clone)]
struct MoveTarget {
	key: String,
	transform: TransformDto,
	parent_inverse: Mat4,
}
struct TransformCache {
	generation: u64,
	revision: u64,
	selection: Vec<String>,
	targets: Vec<TransformTarget>,
	active_world: Mat4,
	move_targets: Vec<MoveTarget>,
}

pub(crate) struct ViewportInteraction {
	pub camera: EditorCamera,
	pub settings: ViewportSettingsDto,
	generation: Option<u64>,
	scene_id: String,
	bookmarks: HashMap<String, EditorCamera>,
	targets: Vec<PickTarget>,
	target_indices: HashMap<String, usize>,
	target_revision: Option<(u64, u64)>,
	initial_frame: bool,
	navigation: Option<Navigation>,
	drag: Option<GizmoDrag>,
	drag_visual: Option<Gizmo>,
	drag_delta: Vec3,
	drag_generation: Option<u64>,
	move_targets: Vec<MoveTarget>,
	transform_cache: RefCell<Option<TransformCache>>,
	collider_visuals: collider_visuals::ColliderVisuals,
	click: Option<Click>,
	placement: Option<Placement>,
	import: Option<AssetTicket>,
	keys: HashSet<String>,
	play_gamepad: runtime::input::GamepadOwner,
	revision: u64,
	focused: bool,
	error: Option<String>,
	document_changed: bool,
	playing: bool,
	camera_preview_active: bool,
	game_status_revision: Option<(u64, u64)>,
	game_status: Option<bridge::editor::PlayStatusDto>,
	orbit_status: Option<OrbitStatusDto>,
	scene_warming_result: Option<SceneWarmingResultDto>,
	last_extent: [u32; 2],
	last_dpi: f64,
	hovered: Option<String>,
	hover_handle: Option<tools::Handle>,
	preview: Option<ViewportTransformPreviewDto>,
}

impl Default for ViewportInteraction {
	fn default() -> Self {
		Self {
			camera: EditorCamera::default(),
			settings: ViewportSettingsDto::default(),
			generation: None,
			scene_id: String::new(),
			bookmarks: HashMap::new(),
			targets: Vec::new(),
			target_indices: HashMap::new(),
			target_revision: None,
			initial_frame: true,
			navigation: None,
			drag: None,
			drag_visual: None,
			drag_delta: Vec3::ZERO,
			drag_generation: None,
			move_targets: Vec::new(),
			transform_cache: RefCell::new(None),
			collider_visuals: Default::default(),
			click: None,
			placement: None,
			import: None,
			keys: HashSet::new(),
			play_gamepad: runtime::input::GamepadOwner::default(),
			revision: 0,
			focused: false,
			error: None,
			document_changed: false,
			playing: false,
			camera_preview_active: false,
			game_status_revision: None,
			game_status: None,
			orbit_status: None,
			scene_warming_result: None,
			last_extent: [0; 2],
			last_dpi: 1.0,
			hovered: None,
			hover_handle: None,
			preview: None,
		}
	}
}

impl ViewportInteraction {
	fn changed(&mut self) {
		self.revision = self.revision.saturating_add(1);
	}

	pub fn revision(&self) -> u64 {
		self.revision
	}

	pub fn state(&self) -> ViewportEditorStateDto {
		ViewportEditorStateDto {
			settings: self.settings.clone(),
			camera_position: self.camera.position().to_array(),
			camera_target: self.camera.pivot.to_array(),
			camera_distance: self.camera.distance,
			revision: self.revision,
			workspace_generation: self.generation,
			gesture: if self.drag.is_some() {
				Some("transform".into())
			} else if self.import.is_some() {
				Some("asset_loading".into())
			} else if self.placement.is_some() {
				Some("placement".into())
			} else if self.is_flying() {
				Some("fly".into())
			} else if self.navigation.is_some() {
				Some("navigation".into())
			} else {
				None
			},
			placement: self
				.placement
				.as_ref()
				.map(|placement| ViewportPlacementDto {
					path: placement.path.clone(),
					position: placement.position.map(|position| position.to_array()),
				}),
			focused: self.focused,
			error: self.error.clone(),
			collider_error: self.collider_visuals.error().map(str::to_owned),
			transform_preview: self.preview.clone(),
			game_status: self.game_status.clone(),
			orbit_status: self.orbit_status.clone(),
			scene_warming_result: self.scene_warming_result.clone(),
		}
	}

	pub fn report(&mut self, error: impl Into<String>) {
		self.error = Some(error.into());
		self.changed();
	}

	pub fn clear_error(&mut self) {
		if self.error.take().is_some() {
			self.changed();
		}
	}

	pub fn take_document_changed(&mut self) -> bool {
		std::mem::take(&mut self.document_changed)
	}

	fn sync_colliders(&mut self, workspace: &EditorWorkspace) {
		if self.collider_visuals.sync(
			workspace,
			self.settings.colliders && self.settings.view == ViewportView::Scene,
		) {
			self.changed();
		}
	}

	pub(crate) fn sync_scene_warming(&mut self, workspace: &EditorWorkspace) {
		let session = workspace.backend.session();
		let latest = workspace
			.scene_warming
			.latest(session.active_scene_id().as_str(), session.scene_revision());
		if self.scene_warming_result.as_ref() != latest {
			self.scene_warming_result = latest.cloned();
			self.changed();
		}
	}

	pub fn sync(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		let generation = workspace.generation;
		let scene_id = workspace
			.backend
			.session()
			.active_scene_id()
			.as_str()
			.to_owned();
		let playing = workspace.backend.session().is_playing();
		if self.generation != Some(generation) || self.scene_id != scene_id {
			// Project replacement is already guarded against active transactions.
			self.navigation = None;
			self.drag = None;
			self.drag_visual = None;
			self.drag_generation = None;
			self.click = None;
			self.placement = None;
			self.import = None;
			self.keys.clear();
			self.play_gamepad.reset();
			if self.generation == Some(generation) {
				if self.bookmarks.len() >= MAX_VIEW_BOOKMARKS {
					self.bookmarks.clear();
				}
				self.bookmarks
					.insert(self.scene_id.clone(), self.camera.clone());
			} else {
				self.bookmarks.clear();
			}
			self.camera = self.bookmarks.get(&scene_id).cloned().unwrap_or_default();
			self.initial_frame = !self.bookmarks.contains_key(&scene_id);
			self.generation = Some(generation);
			self.scene_id = scene_id;
			self.target_revision = None;
			self.targets.clear();
			self.target_indices.clear();
			self.move_targets.clear();
			self.transform_cache.borrow_mut().take();
			self.error = None;
			self.changed();
			self.hovered = None;
			self.hover_handle = None;
			self.preview = None;
		}
		if self.playing != playing {
			self.cancel(workspace)?;
			self.playing = playing;
			self.error = None;
			self.changed();
		}
		let preview_active = workspace.active_camera_preview().is_some();
		if self.camera_preview_active != preview_active {
			self.camera_preview_active = preview_active;
			// A pending click or fly gesture belongs to the previous camera.
			self.end_navigation();
			self.click = None;
			self.keys.clear();
			self.play_gamepad.reset();
			self.hovered = None;
			self.hover_handle = None;
			self.changed();
		}
		if self.game_status_revision != Some((generation, workspace.backend.session().revision())) {
			self.sync_game_status(workspace);
		}
		self.sync_scene_warming(workspace);
		self.sync_colliders(workspace);
		Ok(())
	}

	/// Called after the rendered scene has synchronized, never on every camera move.
	pub fn synchronize_targets(
		&mut self,
		scene: &Scene,
		generation: u64,
		scene_revision: u64,
		extent: [u32; 2],
	) -> Result<(), String> {
		if self.target_revision == Some((generation, scene_revision)) {
			return Ok(());
		}
		self.targets.clear();
		self.target_indices.clear();
		for id in scene.entity_ids() {
			let determinant = scene.world_matrix(id)?.determinant();
			if !scene.is_renderable(id) || !determinant.is_finite() || determinant == 0.0 {
				continue;
			}
			let (Some(key), Some(bounds)) = (scene.key(id), scene.world_bounds(id)?) else {
				continue;
			};
			self.target_indices
				.insert(key.as_str().to_owned(), self.targets.len());
			self.targets.push(PickTarget {
				key: key.as_str().to_owned(),
				bounds,
			});
		}
		self.target_revision = Some((generation, scene_revision));
		if self.initial_frame {
			if let Some(bounds) = self
				.targets
				.iter()
				.map(|target| target.bounds)
				.reduce(Bounds3::union)
			{
				self.camera.orient(ViewportOrientation::Free);
				self.camera
					.frame(bounds, extent[0].max(1) as f32 / extent[1].max(1) as f32);
			}
			self.initial_frame = false;
			self.changed();
		}
		Ok(())
	}

	pub fn geometry_changed(
		&mut self,
		workspace: &mut EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<(), String> {
		if self.last_extent != extent || self.last_dpi != dpi {
			self.cancel_transform(workspace)?;
			self.navigation = None;
			self.click = None;
			self.keys.clear();
			if self.game_keyboard_active() {
				self.clear_play_input(workspace);
			}
			if self.import.is_none() {
				if extent.contains(&0) {
					self.placement = None;
				} else if let Some(placement) = self.placement.as_ref() {
					// A WebView asset drag can resize the aperture without losing capture.
					self.update_placement(placement.normalized, extent);
				}
			}
			self.last_extent = extent;
			self.last_dpi = dpi;
			self.changed();
		}
		Ok(())
	}

	fn execute(&mut self, workspace: &mut EditorWorkspace, command: Command) -> Result<(), String> {
		let preview = self.drag.is_some() && matches!(&command, Command::SetTransforms { .. });
		let revision = workspace.backend.session().revision();
		workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: revision,
				command,
			})
			.map_err(|error| error.to_string())?;
		if preview {
			let session = workspace.backend.session();
			self.preview = session.selection().first().and_then(|key| {
				session
					.authored_document()
					.entities
					.iter()
					.find(|entity| &entity.key == key)
					.map(|entity| ViewportTransformPreviewDto {
						key: key.as_str().to_owned(),
						transform: TransformDto {
							position: entity.transform.position,
							rotation: entity.transform.rotation,
							scale: entity.transform.scale,
						},
					})
			});
			self.changed();
		} else {
			self.document_changed = true;
		}
		self.sync_colliders(workspace);
		Ok(())
	}

	fn can_author(&self, workspace: &EditorWorkspace) -> bool {
		self.settings.view == ViewportView::Scene
			&& !self.camera_preview_active
			&& !workspace.backend.session().is_playing()
			&& self.import.is_none()
	}

	pub fn action(
		&mut self,
		action: ViewportActionDto,
		workspace: &mut EditorWorkspace,
		extent: [u32; 2],
	) -> Result<(), String> {
		self.sync(workspace)?;
		action.validate()?;
		if self.camera_preview_active
			&& !matches!(
				action,
				ViewportActionDto::Cancel | ViewportActionDto::CancelPlacement
			) {
			return Err("Close the camera preview to use Scene tools".into());
		}
		self.error = None;
		// A settings boundary resolves its captured gesture before changing axes or projection.
		if !matches!(
			&action,
			ViewportActionDto::UpdatePlacement { .. }
				| ViewportActionDto::CommitPlacement
				| ViewportActionDto::CancelPlacement
		) && self.drag.is_some()
		{
			self.cancel_transform(workspace)?;
		}
		match action {
			ViewportActionDto::RestartRun { .. } => {
				return Err("Restart Run must be prepared by the native viewport worker".into());
			}
			ViewportActionDto::SetTool { tool } => self.settings.tool = tool,
			ViewportActionDto::SetSpace { space } => self.settings.space = space,
			ViewportActionDto::SetPivot { pivot } => self.settings.pivot = pivot,
			ViewportActionDto::SetGrid { visible } => self.settings.grid = visible,
			ViewportActionDto::SetColliders { visible } => self.settings.colliders = visible,
			ViewportActionDto::SetGridSpacing { spacing } => self.settings.grid_spacing = spacing,
			ViewportActionDto::SetSnap { enabled } => self.settings.snap = enabled,
			ViewportActionDto::SetSnapIncrements {
				translation,
				rotation_degrees,
				scale,
			} => {
				self.settings.move_snap = translation;
				self.settings.rotation_snap_degrees = rotation_degrees;
				self.settings.scale_snap = scale;
			}
			ViewportActionDto::SetProjection { projection } => {
				self.end_navigation();
				self.settings.projection = projection;
			}
			ViewportActionDto::SetPlane { plane } => self.settings.plane = plane,
			ViewportActionDto::SetView { view } => {
				if self.settings.view != view {
					self.cancel(workspace)?;
					workspace.backend.clear_play_input();
					self.settings.view = view;
				}
			}
			ViewportActionDto::Orient { orientation } => {
				self.end_navigation();
				self.camera.orient(orientation);
				self.settings.projection = if orientation == ViewportOrientation::Free {
					ViewportProjection::Perspective
				} else {
					ViewportProjection::Orthographic
				};
				self.settings.plane = match orientation {
					ViewportOrientation::Front | ViewportOrientation::Back => ViewportPlane::Xy,
					ViewportOrientation::Left | ViewportOrientation::Right => ViewportPlane::Yz,
					_ => ViewportPlane::Xz,
				};
			}
			ViewportActionDto::Zoom { factor } => self.camera.zoom(factor),
			ViewportActionDto::FrameSelection => self.frame_selection(workspace, extent)?,
			ViewportActionDto::ResetView => {
				self.camera = EditorCamera::default();
				self.initial_frame = true;
				self.target_revision = None;
			}
			ViewportActionDto::Cancel => self.cancel(workspace)?,
			ViewportActionDto::BeginPlacement { path } => {
				if !self.can_author(workspace) || workspace.backend.session().has_transaction() {
					return Err(
						"Stop Play and finish the current edit before placing a model".into(),
					);
				}
				let extension = std::path::Path::new(&path)
					.extension()
					.and_then(|extension| extension.to_str())
					.unwrap_or("");
				if !extension.eq_ignore_ascii_case("gltf") && !extension.eq_ignore_ascii_case("glb")
				{
					return Err("Only catalogued glTF/GLB models can be placed".into());
				}
				let full = workspace
					.asset_root
					.join(&path)
					.canonicalize()
					.map_err(|error| error.to_string())?;
				if !full.starts_with(&workspace.asset_root) || !full.is_file() {
					return Err("Model must belong to the open project's Assets folder".into());
				}
				self.placement = Some(Placement {
					path,
					normalized: None,
					position: None,
				});
			}
			ViewportActionDto::UpdatePlacement { normalized } => {
				self.update_placement(normalized, extent);
			}
			ViewportActionDto::CommitPlacement => {
				if !self.can_author(workspace) {
					return Err(
						"Model placement is unavailable while playing or loading another model"
							.into(),
					);
				}
				let placement = self
					.placement
					.as_ref()
					.ok_or("No model placement is active")?;
				let position = placement.position.ok_or(
					"Move the model over a valid surface or construction plane before dropping",
				)?;
				let revision = workspace.backend.session().revision();
				self.import = Some(workspace.submit_import(
					placement.path.clone(),
					position.to_array(),
					revision,
					128 * 1024 * 1024,
				)?);
			}
			ViewportActionDto::CancelPlacement => self.cancel_placement(workspace)?,
		}
		self.sync_colliders(workspace);
		self.changed();
		Ok(())
	}

	pub fn cancel(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		self.cancel_transform(workspace)?;
		self.cancel_placement(workspace)?;
		self.navigation = None;
		self.click = None;
		self.clear_play_input(workspace);
		self.changed();
		Ok(())
	}

	pub fn input(
		&mut self,
		input: &ViewportInputDto,
		workspace: &mut EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<(), String> {
		self.sync(workspace)?;
		if matches!(input.kind.as_str(), "focused" | "blurred") {
			return self.set_keyboard_focus(input.kind == "focused", workspace);
		}
		if self.camera_preview_active || extent.contains(&0) {
			return Ok(());
		}
		let point = input.position.unwrap_or([0.0; 2]);
		let cursor = Vec2::new(point[0] as f32, point[1] as f32);
		let ctrl = input.modifiers.control || input.modifiers.super_key;
		match input.kind.as_str() {
			"pointer_down" if self.settings.view == ViewportView::Scene => {
				let button = input.button.as_deref().unwrap_or("");
				if self.persistent_fly() && matches!(button, "left" | "right") {
					if button == "right" {
						self.restore_walk_camera();
					}
					self.end_navigation();
					return Ok(());
				}
				if self.drag.is_some() || self.navigation.is_some() || self.click.is_some() {
					return Ok(());
				}
				if self.placement.is_some() && self.import.is_none() {
					return Ok(());
				}
				self.error = None;
				let navigation = match button {
					"middle" => Some((
						if input.modifiers.control {
							NavigationMode::Dolly
						} else if input.modifiers.shift {
							NavigationMode::Pan
						} else {
							NavigationMode::Orbit
						},
						"middle",
					)),
					"left" if input.modifiers.alt => Some((NavigationMode::Orbit, "left")),
					"left" if self.settings.tool == ViewportTool::Hand => {
						Some((NavigationMode::Pan, "left"))
					}
					"right" => Some((NavigationMode::Fly, "right")),
					_ => None,
				};
				if let Some((mode, button)) = navigation {
					self.navigation = Some(Navigation {
						mode,
						previous: cursor,
						button: Some(button),
						walk_origin: None,
					});
					self.changed();
				} else if button == "left" {
					if self.import.is_some() {
						return Ok(());
					}
					if self.can_author(workspace) && !workspace.backend.session().has_transaction()
					{
						if let Some(gizmo) = self.gizmo(workspace, extent, dpi)? {
							if let Some(handle) = gizmo.hit_test(
								&self.camera,
								extent,
								self.settings.projection,
								self.settings.tool,
								point,
								9.0 * dpi as f32,
							) {
								let mut settings = self.settings.clone();
								settings.snap |= ctrl;
								let targets = self.transform_targets(workspace)?;
								let move_targets = targets.move_targets.clone();
								if self.settings.tool == ViewportTool::Move
									&& move_targets.iter().any(|target| {
										!target.parent_inverse.is_finite()
											|| target.parent_inverse.determinant() == 0.0
									}) {
									return Err("Move requires an invertible parent transform; edit the parent's zero scale first".into());
								}
								let drag = GizmoDrag::begin(
									&self.camera,
									extent,
									self.settings.projection,
									point,
									gizmo,
									self.settings.tool,
									handle,
									&settings,
									targets.targets.clone(),
								)?;
								drop(targets);
								self.execute(
									workspace,
									Command::BeginTransaction {
										label: TRANSFORM_LABEL.into(),
									},
								)?;
								self.drag = Some(drag);
								self.drag_visual = Some(gizmo);
								self.drag_delta = Vec3::ZERO;
								self.move_targets = move_targets;
								self.drag_generation = Some(workspace.generation);
								self.changed();
								return Ok(());
							}
						}
					}
					if !workspace.backend.session().has_transaction() {
						self.click = Some(Click {
							start: point,
							current: point,
							selection: workspace
								.backend
								.session()
								.selection()
								.iter()
								.map(|key| key.as_str().to_owned())
								.collect(),
							additive: input.modifiers.shift,
							toggle: ctrl,
						});
					}
				}
			}
			"pointer_move" if self.settings.view == ViewportView::Scene => {
				if let Some(navigation) = self.navigation.as_mut() {
					let delta = cursor - std::mem::replace(&mut navigation.previous, cursor);
					let logical = delta / dpi as f32;
					match navigation.mode {
						NavigationMode::Orbit => self.camera.orbit(logical),
						NavigationMode::Pan => {
							self.camera.pan(delta, extent, self.settings.projection)
						}
						NavigationMode::Dolly => {
							self.camera.zoom((logical.y * 0.009).clamp(-2.0, 2.0).exp())
						}
						NavigationMode::Fly => return Ok(()),
					}
					self.changed();
				}
				if let Some(drag) = self.drag.as_mut() {
					let (edits, space) = if drag.tool() == ViewportTool::Move {
						let delta = drag.translation_delta(point)?;
						self.drag_delta = delta;
						let edits = self
							.move_targets
							.iter()
							.map(|target| {
								let mut transform = target.transform.clone();
								transform.position = (Vec3::from_array(transform.position)
									+ target.parent_inverse.transform_vector3(delta))
								.to_array();
								TransformEditDto {
									key: target.key.clone(),
									transform,
								}
							})
							.collect();
						(edits, TransformSpace::Local)
					} else {
						(drag.preview(point)?, TransformSpace::World)
					};
					if !edits.is_empty() {
						self.execute(
							workspace,
							Command::SetTransforms {
								edits,
								space,
								snap: false,
							},
						)?;
					}
				}
				if let Some(click) = self.click.as_mut() {
					click.current = point;
					self.changed();
				}
				if !self.captured() && self.placement.is_none() {
					let hover_handle = self.gizmo(workspace, extent, dpi)?.and_then(|gizmo| {
						gizmo.hit_test(
							&self.camera,
							extent,
							self.settings.projection,
							self.settings.tool,
							point,
							9.0 * dpi as f32,
						)
					});
					let hovered = if hover_handle.is_none() {
						tools::pick_nearest(
							self.camera.ray(point, extent, self.settings.projection)?,
							&self.targets,
						)
						.map(|hit| hit.key)
					} else {
						None
					};
					if self.hovered != hovered || self.hover_handle != hover_handle {
						self.hovered = hovered;
						self.hover_handle = hover_handle;
						self.changed();
					}
				}
			}
			"pointer_leave" if !self.captured() => {
				if self.hovered.take().is_some() || self.hover_handle.take().is_some() {
					self.changed();
				}
			}
			"pointer_up" if self.settings.view == ViewportView::Scene => {
				if let Some(navigation) = self.navigation.as_ref() {
					if navigation.button.is_some() && navigation.button == input.button.as_deref() {
						self.end_navigation();
					}
					return Ok(());
				}
				if self.drag.is_some() {
					if input.button.as_deref() != Some("left") {
						return Ok(());
					}
					// The window owner flushes its final pointer sample before this boundary.
					self.execute(workspace, Command::CommitTransaction)?;
					self.drag = None;
					self.drag_visual = None;
					self.drag_generation = None;
					self.move_targets.clear();
					self.preview = None;
					self.changed();
				} else if input.button.as_deref() == Some("left") {
					if let Some(click) = self.click.take() {
						self.select_click(workspace, click, extent, dpi)?;
					}
				}
			}
			"wheel_lines" | "wheel_pixels" if self.settings.view == ViewportView::Scene => {
				if self.drag.is_some() {
					return Ok(());
				}
				let y = input.delta.map_or(0.0, |delta| delta[1]);
				let step = if input.kind == "wheel_lines" {
					y * 0.14
				} else {
					y * 0.002 / dpi
				};
				self.camera.zoom((-step).clamp(-2.0, 2.0).exp() as f32);
				self.changed();
			}
			"key_down" if self.focused && !input.repeat => {
				let code = input.code.as_deref().unwrap_or("");
				if code == "Escape" {
					self.restore_walk_camera();
					self.cancel(workspace)?;
					return self.set_keyboard_focus(false, workspace);
				}
				if self.settings.view == ViewportView::Game {
					if self.game_keyboard_active()
						&& !code.is_empty()
						&& self.keys.insert(code.into())
					{
						self.route_game_input(workspace)?;
					}
					return Ok(());
				}
				if code == "Backquote" && input.modifiers.shift && !ctrl && !input.modifiers.alt {
					if self.persistent_fly() {
						self.end_navigation();
					} else if !self.captured() && self.placement.is_none() {
						self.navigation = Some(Navigation {
							mode: NavigationMode::Fly,
							previous: cursor,
							button: None,
							walk_origin: Some(self.camera.clone()),
						});
						self.changed();
					}
					return Ok(());
				}
				if self.is_flying() {
					if matches!(code, "Enter" | "NumpadEnter") && self.persistent_fly() {
						self.end_navigation();
					} else if matches!(
						code,
						"KeyW"
							| "KeyA" | "KeyS" | "KeyD"
							| "KeyQ" | "KeyE" | "ShiftLeft"
							| "ShiftRight"
					) {
						self.keys.insert(code.into());
					}
					return Ok(());
				}
				if !input.modifiers.alt && !input.modifiers.super_key {
					if let Some(action) = self.scene_shortcut(code, input.modifiers.control) {
						self.end_navigation();
						self.click = None;
						return self.action(action, workspace, extent);
					}
					if !ctrl && self.orbit_key(code, workspace)? {
						return Ok(());
					}
				}
				if ctrl
					&& !workspace.backend.session().has_transaction()
					&& self.can_author(workspace)
				{
					match code {
						"KeyZ" => self.execute(
							workspace,
							if input.modifiers.shift {
								Command::Redo
							} else {
								Command::Undo
							},
						)?,
						"KeyY" => self.execute(workspace, Command::Redo)?,
						"KeyD" => self
							.execute(workspace, Command::DuplicateSelection { offset: [0.0; 3] })?,
						"KeyA" => {
							let keys = workspace
								.backend
								.session()
								.render_document()
								.entities
								.iter()
								.take(4096)
								.map(|entity| entity.key.as_str().to_owned())
								.collect();
							self.execute(workspace, Command::Select { keys })?;
						}
						_ => {}
					}
				} else if !ctrl {
					match code {
						"Delete"
							if self.can_author(workspace)
								&& !workspace.backend.session().has_transaction() =>
						{
							self.execute(workspace, Command::DeleteSelection)?
						}
						_ => return Ok(()),
					}
					self.changed();
				}
			}
			"key_up"
				if input
					.code
					.as_ref()
					.is_some_and(|code| self.keys.remove(code))
					&& self.focused && self.game_keyboard_active() =>
			{
				self.route_game_input(workspace)?;
			}
			_ => {}
		}
		Ok(())
	}
}

fn bounds_corners(bounds: Bounds3) -> [Vec3; 8] {
	let min = bounds.min();
	let max = bounds.max();
	std::array::from_fn(|i| {
		Vec3::new(
			if i & 1 == 0 { min[0] } else { max[0] },
			if i & 2 == 0 { min[1] } else { max[1] },
			if i & 4 == 0 { min[2] } else { max[2] },
		)
	})
}

#[cfg(test)]
#[path = "../tests/unit/viewport_interaction/mod.rs"]
mod interaction_tests;
