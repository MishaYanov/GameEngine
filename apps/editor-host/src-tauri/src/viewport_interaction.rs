//! Native editor interaction ownership, independent of the window and DOM.
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
use runtime::{scene::Scene, Bounds3};

use crate::editor::{AssetTicket, EditorWorkspace};
use crate::viewport_tools::{
	self as tools, EditorCamera, Gizmo, GizmoDrag, PickTarget, TransformTarget,
};

const TRANSFORM_LABEL: &str = "Viewport transform";
const MAX_VIEW_BOOKMARKS: usize = 128;

enum Navigation {
	Orbit(Vec2),
	Pan(Vec2),
	Fly(Vec2),
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
	click: Option<Click>,
	placement: Option<Placement>,
	import: Option<AssetTicket>,
	keys: HashSet<String>,
	revision: u64,
	focused: bool,
	error: Option<String>,
	document_changed: bool,
	playing: bool,
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
			click: None,
			placement: None,
			import: None,
			keys: HashSet::new(),
			revision: 0,
			focused: false,
			error: None,
			document_changed: false,
			playing: false,
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
		self.revision = self.revision.wrapping_add(1);
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
			transform_preview: self.preview.clone(),
		}
	}

	pub fn report(&mut self, error: impl Into<String>) {
		self.error = Some(error.into());
		self.changed();
	}

	pub fn take_document_changed(&mut self) -> bool {
		std::mem::take(&mut self.document_changed)
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
			self.changed();
		}
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
			if self.import.is_none() {
				self.placement = None;
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
		Ok(())
	}

	fn can_author(&self, workspace: &EditorWorkspace) -> bool {
		self.settings.view == ViewportView::Scene
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
		self.error = None;
		// A settings boundary resolves its captured gesture before changing axes or projection.
		if !matches!(
			&action,
			ViewportActionDto::UpdatePlacement { .. }
				| ViewportActionDto::CommitPlacement
				| ViewportActionDto::CancelPlacement
		) {
			if self.drag.is_some() {
				self.cancel_transform(workspace)?;
			}
		}
		match action {
			ViewportActionDto::SetTool { tool } => self.settings.tool = tool,
			ViewportActionDto::SetSpace { space } => self.settings.space = space,
			ViewportActionDto::SetPivot { pivot } => self.settings.pivot = pivot,
			ViewportActionDto::SetGrid { visible } => self.settings.grid = visible,
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
				self.navigation = None;
				self.settings.projection = projection;
			}
			ViewportActionDto::SetPlane { plane } => self.settings.plane = plane,
			ViewportActionDto::SetView { view } => {
				self.cancel(workspace)?;
				self.settings.view = view;
			}
			ViewportActionDto::Orient { orientation } => {
				self.navigation = None;
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
				let point = normalized
					.and_then(|point| {
						self.camera
							.ray(
								[point[0] * extent[0] as f64, point[1] * extent[1] as f64],
								extent,
								self.settings.projection,
							)
							.ok()
					})
					.and_then(|ray| {
						tools::placement_point(
							ray,
							&self.targets,
							self.settings.plane,
							self.settings.snap.then_some(self.settings.move_snap),
						)
					});
				if let Some(placement) = self.placement.as_mut() {
					placement.normalized = normalized;
					placement.position = point;
				}
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
		self.changed();
		Ok(())
	}

	pub fn drain_imports(&mut self, workspace: &mut EditorWorkspace) {
		workspace.drain_asset_jobs();
		if self.import.is_none() {
			return;
		}
		// The UI notifier may have adopted the job first. Completions are retained
		// by the workspace so each consumer can reconcile its own ticket once.
		for completion in workspace.recent_completions() {
			if self.import.as_ref().is_some_and(|ticket| {
				ticket.job_id == completion.job_id
					&& ticket.workspace_generation == completion.workspace_generation
			}) {
				self.import = None;
				self.placement = None;
				self.error = completion.error;
				self.document_changed |= completion.accepted;
				self.changed();
			}
		}
	}

	fn cancel_placement(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		if let Some(ticket) = self.import.take() {
			if ticket.workspace_generation == workspace.generation {
				let _ = workspace.cancel_import(ticket.job_id, ticket.workspace_generation)?;
			}
		}
		self.placement = None;
		Ok(())
	}

	fn cancel_transform(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		if self.drag.is_some()
			&& self.drag_generation == Some(workspace.generation)
			&& workspace.backend.session().state().transaction.as_deref() == Some(TRANSFORM_LABEL)
		{
			self.execute(workspace, Command::CancelTransaction)?;
		}
		self.drag = None;
		self.drag_visual = None;
		self.drag_generation = None;
		self.move_targets.clear();
		self.preview = None;
		Ok(())
	}

	pub fn cancel(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		self.cancel_transform(workspace)?;
		self.cancel_placement(workspace)?;
		self.navigation = None;
		self.click = None;
		self.keys.clear();
		self.changed();
		Ok(())
	}

	fn selected_bounds(&self, workspace: &EditorWorkspace) -> Option<Bounds3> {
		workspace
			.backend
			.session()
			.selection()
			.iter()
			.filter_map(|key| {
				self.target_indices
					.get(key.as_str())
					.map(|&index| self.targets[index].bounds)
			})
			.reduce(Bounds3::union)
	}

	fn frame_selection(
		&mut self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
	) -> Result<(), String> {
		let bounds = self
			.selected_bounds(workspace)
			.or_else(|| {
				self.targets
					.iter()
					.map(|target| target.bounds)
					.reduce(Bounds3::union)
			})
			.ok_or("There is no renderable entity to frame")?;
		self.camera
			.frame(bounds, extent[0].max(1) as f32 / extent[1].max(1) as f32);
		Ok(())
	}

	fn transform_targets(
		&self,
		workspace: &EditorWorkspace,
	) -> Result<Ref<'_, TransformCache>, String> {
		let session = workspace.backend.session();
		let valid = self.transform_cache.borrow().as_ref().is_some_and(|cache| {
			cache.generation == workspace.generation
				&& cache.revision == session.scene_revision()
				&& cache.selection.len() == session.selection().len()
				&& cache
					.selection
					.iter()
					.zip(session.selection())
					.all(|(a, b)| a == b.as_str())
		});
		if valid {
			return Ok(Ref::map(self.transform_cache.borrow(), |cache| {
				cache.as_ref().expect("validated cache")
			}));
		}
		let document = session.authored_document();
		let matrices = document.world_matrices()?;
		let worlds: HashMap<_, _> = document
			.entities
			.iter()
			.zip(matrices.iter())
			.map(|(entity, matrix)| (entity.key.as_str(), *matrix))
			.collect();
		let selected: HashSet<_> = session.selection().iter().map(|key| key.as_str()).collect();
		let parents: HashMap<_, _> = document
			.entities
			.iter()
			.map(|entity| {
				(
					entity.key.as_str(),
					entity.parent.as_ref().map(|parent| parent.as_str()),
				)
			})
			.collect();
		let roots: Vec<_> = document
			.entities
			.iter()
			.zip(matrices)
			.filter(|(entity, _)| {
				if !selected.contains(entity.key.as_str()) {
					return false;
				}
				let mut parent = entity.parent.as_ref().map(|parent| parent.as_str());
				while let Some(key) = parent {
					if selected.contains(key) {
						return false;
					}
					parent = parents.get(key).copied().flatten();
				}
				true
			})
			.collect();
		let mut targets: Vec<_> = roots
			.iter()
			.map(|(entity, world)| TransformTarget {
				key: entity.key.as_str().to_owned(),
				world: *world,
			})
			.collect();
		let move_targets = roots
			.iter()
			.map(|(entity, _)| {
				let parent = entity
					.parent
					.as_ref()
					.and_then(|key| worlds.get(key.as_str()))
					.copied()
					.unwrap_or(Mat4::IDENTITY);
				let determinant = parent.determinant();
				MoveTarget {
					key: entity.key.as_str().to_owned(),
					transform: TransformDto {
						position: entity.transform.position,
						rotation: entity.transform.rotation,
						scale: entity.transform.scale,
					},
					parent_inverse: if determinant.is_finite() && determinant != 0.0 {
						parent.inverse()
					} else {
						Mat4::ZERO
					},
				}
			})
			.collect();
		let order: HashMap<_, _> = session
			.selection()
			.iter()
			.enumerate()
			.map(|(index, key)| (key.as_str(), index))
			.collect();
		targets.sort_by_key(|target| {
			order
				.get(target.key.as_str())
				.copied()
				.unwrap_or(usize::MAX)
		});
		let active_world = session
			.selection()
			.first()
			.and_then(|key| worlds.get(key.as_str()))
			.copied()
			.unwrap_or(Mat4::IDENTITY);
		*self.transform_cache.borrow_mut() = Some(TransformCache {
			generation: workspace.generation,
			revision: session.scene_revision(),
			selection: session
				.selection()
				.iter()
				.map(|key| key.as_str().to_owned())
				.collect(),
			targets,
			active_world,
			move_targets,
		});
		Ok(Ref::map(self.transform_cache.borrow(), |cache| {
			cache.as_ref().expect("initialized cache")
		}))
	}

	fn gizmo(
		&self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<Option<Gizmo>, String> {
		if !self.can_author(workspace)
			|| self.settings.tool == ViewportTool::Hand
			|| workspace.backend.session().selection().is_empty()
		{
			return Ok(None);
		}
		// A captured gesture keeps its original basis. Avoid another scene-wide
		// world-matrix/map rebuild after each authored transform preview.
		if self.drag.is_some() {
			return Ok(self.drag_visual.map(|mut gizmo| {
				gizmo.pivot += self.drag_delta;
				gizmo
			}));
		}
		let targets = self.transform_targets(workspace)?;
		if targets.targets.is_empty() {
			return Ok(None);
		}
		let active = targets.active_world;
		let pivot = if self.settings.pivot == ViewportPivot::Center {
			self.selected_bounds(workspace)
				.map(|bounds| {
					(Vec3::from_array(bounds.min()) + Vec3::from_array(bounds.max())) * 0.5
				})
				.unwrap_or(active.w_axis.truncate())
		} else {
			active.w_axis.truncate()
		};
		let rotation = active.to_scale_rotation_translation().1;
		let basis = if self.settings.space == ViewportSpace::Local && rotation.is_finite() {
			rotation
		} else {
			Quat::IDENTITY
		};
		Ok(Some(Gizmo::new(
			pivot,
			basis,
			self.camera
				.gizmo_size(pivot, extent, self.settings.projection, 80.0 * dpi as f32),
		)))
	}

	pub fn input(
		&mut self,
		input: &ViewportInputDto,
		workspace: &mut EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<(), String> {
		self.sync(workspace)?;
		if extent.contains(&0) {
			return Ok(());
		}
		let point = input.position.unwrap_or([0.0; 2]);
		let cursor = Vec2::new(point[0] as f32, point[1] as f32);
		let ctrl = input.modifiers.control || input.modifiers.super_key;
		match input.kind.as_str() {
			"focused" => {
				self.focused = true;
				self.changed();
			}
			"blurred" => {
				// Internal asset drags are captured by the WebView, which owns
				// their app-blur cancellation. Losing child focus alone is normal.
				self.focused = false;
				self.cancel_transform(workspace)?;
				self.navigation = None;
				self.click = None;
				self.keys.clear();
				workspace.backend.clear_play_input();
				self.changed();
			}
			"pointer_down" if self.settings.view == ViewportView::Game => {
				self.focused = true;
				self.changed();
			}
			"pointer_down" if self.settings.view == ViewportView::Scene => {
				if self.drag.is_some() || self.navigation.is_some() || self.click.is_some() {
					return Ok(());
				}
				if self.placement.is_some() && self.import.is_none() {
					return Ok(());
				}
				self.error = None;
				let button = input.button.as_deref().unwrap_or("");
				if button == "left" && input.modifiers.alt {
					self.navigation = Some(Navigation::Orbit(cursor));
					self.changed();
				} else if button == "middle"
					|| (button == "left" && self.settings.tool == ViewportTool::Hand)
				{
					self.navigation = Some(Navigation::Pan(cursor));
					self.changed();
				} else if button == "right" {
					self.navigation = Some(Navigation::Fly(cursor));
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
				match self.navigation.as_mut() {
					Some(Navigation::Orbit(previous)) | Some(Navigation::Fly(previous)) => {
						let delta = (cursor - *previous) / (dpi as f32);
						*previous = cursor;
						self.camera.orbit(delta);
						self.changed();
					}
					Some(Navigation::Pan(previous)) => {
						let delta = cursor - *previous;
						*previous = cursor;
						self.camera.pan(delta, extent, self.settings.projection);
						self.changed();
					}
					None => {}
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
			"pointer_up" => {
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
				self.navigation = None;
				self.keys.clear();
				self.changed();
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
			"key_down" => {
				let code = input.code.as_deref().unwrap_or("");
				if code == "Escape" {
					self.cancel(workspace)?;
					workspace.backend.clear_play_input();
					self.focused = false;
					self.changed();
					return Ok(());
				}
				if self.settings.view == ViewportView::Game {
					if self.focused && workspace.backend.session().is_playing() {
						self.keys.insert(code.into());
						self.route_game_input(workspace)?;
					}
					return Ok(());
				}
				if matches!(self.navigation, Some(Navigation::Fly(_))) {
					self.keys.insert(code.into());
					return Ok(());
				}
				if input.repeat {
					return Ok(());
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
					if matches!(code, "KeyQ" | "KeyW" | "KeyE" | "KeyR" | "KeyF") {
						self.cancel_transform(workspace)?;
						self.navigation = None;
						self.click = None;
					}
					match code {
						"KeyQ" => self.settings.tool = ViewportTool::Hand,
						"KeyW" => self.settings.tool = ViewportTool::Move,
						"KeyE" => self.settings.tool = ViewportTool::Rotate,
						"KeyR" => self.settings.tool = ViewportTool::Scale,
						"KeyF" => self.frame_selection(workspace, extent)?,
						"Delete"
							if self.can_author(workspace)
								&& !workspace.backend.session().has_transaction() =>
						{
							self.execute(workspace, Command::DeleteSelection)?
						}
						_ => {}
					}
					self.changed();
				}
			}
			"key_up" => {
				if let Some(code) = input.code.as_ref() {
					self.keys.remove(code);
				}
				if self.settings.view == ViewportView::Game
					&& workspace.backend.session().is_playing()
				{
					self.route_game_input(workspace)?;
				}
			}
			_ => {}
		}
		Ok(())
	}

	fn select_click(
		&mut self,
		workspace: &mut EditorWorkspace,
		click: Click,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<(), String> {
		let dx = click.current[0] - click.start[0];
		let dy = click.current[1] - click.start[1];
		let hits: Vec<String> = if dx.hypot(dy) > 4.0 * dpi {
			let matrix = self
				.camera
				.view_projection(extent, self.settings.projection)?;
			let min = Vec2::new(
				click.start[0].min(click.current[0]) as f32,
				click.start[1].min(click.current[1]) as f32,
			);
			let max = Vec2::new(
				click.start[0].max(click.current[0]) as f32,
				click.start[1].max(click.current[1]) as f32,
			);
			self.targets
				.iter()
				.filter(|target| {
					let mut bmin = Vec2::splat(f32::INFINITY);
					let mut bmax = Vec2::splat(f32::NEG_INFINITY);
					let mut visible = false;
					for point in bounds_corners(target.bounds) {
						if let Some(projected) = tools::project(matrix, extent, point) {
							bmin = bmin.min(projected);
							bmax = bmax.max(projected);
							visible = true;
						}
					}
					if !visible {
						return false;
					}
					if dx >= 0.0 {
						(bmin.cmpge(min) & bmax.cmple(max)).all()
					} else {
						(bmax.cmpge(min) & bmin.cmple(max)).all()
					}
				})
				.take(4096)
				.map(|target| target.key.clone())
				.collect()
		} else {
			tools::pick_nearest(
				self.camera
					.ray(click.current, extent, self.settings.projection)?,
				&self.targets,
			)
			.map(|hit| vec![hit.key])
			.unwrap_or_default()
		};
		let mut keys = if click.additive || click.toggle {
			click.selection
		} else {
			Vec::new()
		};
		for hit in hits {
			if click.toggle && keys.contains(&hit) {
				keys.retain(|key| key != &hit);
			} else if !keys.contains(&hit) && keys.len() < 4096 {
				keys.push(hit);
			}
		}
		self.execute(workspace, Command::Select { keys })?;
		Ok(())
	}

	fn fly_movement(&self) -> Vec3 {
		let down = |key: &str| if self.keys.contains(key) { 1.0 } else { 0.0 };
		Vec3::new(
			down("KeyD") - down("KeyA"),
			down("KeyE") - down("KeyQ"),
			down("KeyW") - down("KeyS"),
		)
	}
	pub fn needs_camera_tick(&self) -> bool {
		matches!(self.navigation, Some(Navigation::Fly(_))) && self.fly_movement() != Vec3::ZERO
	}
	pub fn camera_tick(&mut self, seconds: f32) {
		if !self.needs_camera_tick() {
			return;
		}
		let movement = self.fly_movement();
		let speed = if self.keys.contains("ShiftLeft") || self.keys.contains("ShiftRight") {
			15.0
		} else {
			5.0
		};
		self.camera.fly(movement, seconds.min(0.05), speed);
		self.changed();
	}

	pub fn captured(&self) -> bool {
		self.navigation.is_some() || self.drag.is_some() || self.click.is_some()
	}

	fn route_game_input(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		// Match the player's shared default XY movement vector contract. The
		// game module decides how this input affects the controlled world.
		let mut input = runtime::input::InputState::default();
		for (code, key) in [
			("KeyW", runtime::input::Key::W),
			("KeyA", runtime::input::Key::A),
			("KeyS", runtime::input::Key::S),
			("KeyD", runtime::input::Key::D),
		] {
			if self.keys.contains(code) {
				input.press(key);
			}
		}
		let direction = runtime::input::MovementBindings::default().direction(&input);
		let revision = workspace.backend.session().revision();
		workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: revision,
				command: Command::SetPlayInput {
					movement: [direction[0], direction[1], 0.0],
				},
			})
			.map_err(|error| error.to_string())?;
		Ok(())
	}

	pub fn visuals(
		&self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<EditorVisuals, String> {
		if self.settings.view == ViewportView::Game {
			return Ok(EditorVisuals::default());
		}
		let mut visuals = EditorVisuals::default();
		if self.settings.grid {
			let (normal, tangent, _) = tools::plane_basis(self.settings.plane);
			visuals.grid = Some(EditorGrid {
				origin: [0.0; 3],
				normal: normal.to_array(),
				tangent: tangent.to_array(),
				spacing: tools::adaptive_grid_spacing(
					self.settings.grid_spacing,
					self.camera.units_per_pixel(
						self.camera.pivot,
						extent,
						self.settings.projection,
					),
				),
				opacity: 0.38,
				fade_distance: (self.camera.distance * 15.0).clamp(100.0, 100_000.0),
			});
		}
		let selection = workspace.backend.session().selection();
		if let Some(index) = self
			.hovered
			.as_ref()
			.and_then(|key| self.target_indices.get(key))
		{
			if !selection
				.iter()
				.any(|key| key.as_str() == self.targets[*index].key)
			{
				box_lines(
					&mut visuals,
					self.targets[*index].bounds,
					[0.7, 0.8, 1.0, 0.6],
					dpi as f32,
				);
			}
		}
		if selection.len() > 1024 {
			if let Some(bounds) = self.selected_bounds(workspace) {
				box_lines(
					&mut visuals,
					bounds,
					[1.0, 0.72, 0.22, 1.0],
					1.5 * dpi as f32,
				);
			}
		} else {
			for key in selection {
				if let Some(&index) = self.target_indices.get(key.as_str()) {
					box_lines(
						&mut visuals,
						self.targets[index].bounds,
						[1.0, 0.72, 0.22, 1.0],
						1.5 * dpi as f32,
					);
				}
			}
		}
		if let Some(gizmo) = self.gizmo(workspace, extent, dpi)? {
			for axis in gizmo.axes {
				let half = axis * gizmo.size * 0.045;
				visuals.lines.push(EditorLine {
					start: (gizmo.pivot - half).to_array(),
					end: (gizmo.pivot + half).to_array(),
					color: [1.0, 0.86, 0.4, 1.0],
					width: 3.0 * dpi as f32,
					depth_test: false,
				});
			}
			for segment in gizmo.segments(self.settings.tool) {
				let color = if self.hover_handle == Some(segment.handle) {
					[1.0, 0.86, 0.4, 1.0]
				} else {
					match segment.axis {
						0 => [0.94, 0.26, 0.24, 1.0],
						1 => [0.36, 0.85, 0.49, 1.0],
						2 => [0.3, 0.57, 1.0, 1.0],
						_ => [1.0, 0.8, 0.3, 1.0],
					}
				};
				visuals.lines.push(EditorLine {
					start: segment.a.to_array(),
					end: segment.b.to_array(),
					color,
					width: 2.2 * dpi as f32,
					depth_test: false,
				});
			}
		}
		if let Some(position) = self
			.placement
			.as_ref()
			.and_then(|placement| placement.position)
		{
			let bounds = Bounds3::new(
				(position - Vec3::new(0.5, 0.0, 0.5)).to_array(),
				(position + Vec3::new(0.5, 1.0, 0.5)).to_array(),
			)?;
			box_lines(&mut visuals, bounds, [0.4, 1.0, 0.7, 0.9], 2.0 * dpi as f32);
		}
		if let Some(click) = self.click.as_ref() {
			if (click.current[0] - click.start[0]).hypot(click.current[1] - click.start[1])
				> 4.0 * dpi
			{
				let matrix = self
					.camera
					.view_projection(extent, self.settings.projection)?
					.inverse();
				let points = [
					[click.start[0], click.start[1]],
					[click.current[0], click.start[1]],
					click.current,
					[click.start[0], click.current[1]],
				];
				let world = points.map(|point| {
					let p = matrix
						* Vec4::new(
							(point[0] / extent[0] as f64 * 2.0 - 1.0) as f32,
							(point[1] / extent[1] as f64 * 2.0 - 1.0) as f32,
							0.1,
							1.0,
						);
					p.truncate() / p.w
				});
				for i in 0..4 {
					visuals.lines.push(EditorLine {
						start: world[i].to_array(),
						end: world[(i + 1) % 4].to_array(),
						color: [0.5, 0.75, 1.0, 1.0],
						width: 1.5 * dpi as f32,
						depth_test: false,
					});
				}
			}
		}
		Ok(visuals)
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
fn box_lines(visuals: &mut EditorVisuals, bounds: Bounds3, color: [f32; 4], width: f32) {
	let corners = bounds_corners(bounds);
	for i in 0..8 {
		for axis in [1, 2, 4] {
			let other = i ^ axis;
			if other > i {
				visuals.lines.push(EditorLine {
					start: corners[i].to_array(),
					end: corners[other].to_array(),
					color,
					width,
					depth_test: false,
				});
			}
		}
	}
}

#[cfg(test)]
#[path = "viewport_interaction_tests.rs"]
mod interaction_tests;
