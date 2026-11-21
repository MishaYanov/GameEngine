//! Native editor interaction ownership, independent of the window and DOM.
#[path = "collider_visuals.rs"]
mod collider_visuals;

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

/// Identity established by a successful native grab. Every sample retains it until release.
pub(crate) struct GameCameraCapture {
	workspace: u64,
	epoch: u64,
	owner: CameraInputOwner,
}

impl GameCameraCapture {
	pub fn acquire(
		interaction: &ViewportInteraction,
		workspace: &EditorWorkspace,
		native_focused: bool,
		grab: impl FnOnce() -> Result<(), String>,
	) -> Result<Option<Self>, String> {
		if !native_focused
			|| !interaction.keyboard_focused()
			|| !interaction.game_keyboard_active()
			|| interaction.generation != Some(workspace.generation)
		{
			return Ok(None);
		}
		let Some(owner) = workspace.backend.camera_input_owner() else {
			return Ok(None);
		};
		grab()?;
		Ok(Some(Self {
			workspace: workspace.generation,
			epoch: workspace.camera_capture_epoch,
			owner,
		}))
	}

	pub fn current(&self, workspace: &EditorWorkspace) -> bool {
		self.workspace == workspace.generation
			&& self.epoch == workspace.camera_capture_epoch
			&& workspace.backend.owns_camera_input(&self.owner)
	}

	pub fn submit(
		&self,
		workspace: &mut EditorWorkspace,
		intent: CameraIntent,
	) -> Result<(), String> {
		if self.workspace != workspace.generation || self.epoch != workspace.camera_capture_epoch {
			return Err("Game camera capture expired".into());
		}
		workspace.backend.submit_camera_input(
			&self.owner.run_id,
			self.owner.scene_generation,
			intent,
		)
	}
}

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
	revision: u64,
	focused: bool,
	error: Option<String>,
	document_changed: bool,
	playing: bool,
	game_status_revision: Option<(u64, u64)>,
	game_status: Option<bridge::editor::PlayStatusDto>,
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
			revision: 0,
			focused: false,
			error: None,
			document_changed: false,
			playing: false,
			game_status_revision: None,
			game_status: None,
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

	/// Runtime status has its own publication path; it never invalidates an authored snapshot.
	pub fn sync_game_status(&mut self, workspace: &EditorWorkspace) {
		self.game_status_revision =
			Some((workspace.generation, workspace.backend.session().revision()));
		let status = workspace.backend.play_status();
		if self.game_status != status {
			self.game_status = status;
			self.changed();
		}
	}

	pub fn sync_game_status_after_tick(
		&mut self,
		workspace: &EditorWorkspace,
		outcome: &Result<TickOutcome, BackendError>,
	) {
		// Progress, pending scene requests and a pause after rejection can all
		// change the HUD without touching the authored revision.
		let changed = match outcome {
			Ok(outcome) => outcome.progress_changed || outcome.requested_scene.is_some(),
			Err(_) => true,
		};
		if changed {
			self.sync_game_status(workspace);
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
			self.error = None;
			self.changed();
		}
		if self.game_status_revision != Some((generation, workspace.backend.session().revision())) {
			self.sync_game_status(workspace);
		}
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
				workspace.backend.clear_play_input();
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

	fn update_placement(&mut self, normalized: Option<[f64; 2]>, extent: [u32; 2]) {
		let Some(placement) = self.placement.as_mut() else {
			return;
		};
		placement.normalized = normalized;
		placement.position = normalized
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
		if matches!(input.kind.as_str(), "focused" | "blurred") {
			return self.set_keyboard_focus(input.kind == "focused", workspace);
		}
		if extent.contains(&0) {
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

	fn scene_shortcut(&self, code: &str, control: bool) -> Option<ViewportActionDto> {
		use ViewportActionDto::{FrameSelection, Orient, SetProjection, SetTool, Zoom};
		use ViewportOrientation::{Back, Bottom, Front, Left, Right, Top};
		use ViewportProjection::{Orthographic, Perspective};
		use ViewportTool::{Hand, Move, Rotate, Scale};
		Some(match (code, control) {
			("KeyQ", false) => SetTool { tool: Hand },
			("KeyW", false) => SetTool { tool: Move },
			("KeyE", false) => SetTool { tool: Rotate },
			("KeyR", false) => SetTool { tool: Scale },
			("KeyF" | "NumpadDecimal", false) => FrameSelection,
			("NumpadAdd", false) => Zoom { factor: 0.87 },
			("NumpadSubtract", false) => Zoom { factor: 1.0 / 0.87 },
			("Numpad1", false) => Orient { orientation: Front },
			("Numpad1", true) => Orient { orientation: Back },
			("Numpad3", false) => Orient { orientation: Right },
			("Numpad3", true) => Orient { orientation: Left },
			("Numpad7", false) => Orient { orientation: Top },
			("Numpad7", true) => Orient {
				orientation: Bottom,
			},
			("Numpad5", false) => SetProjection {
				projection: match self.settings.projection {
					Perspective => Orthographic,
					Orthographic => Perspective,
				},
			},
			_ => return None,
		})
	}

	fn orbit_key(&mut self, code: &str, workspace: &mut EditorWorkspace) -> Result<bool, String> {
		let step = std::f32::consts::PI / 12.0;
		let (yaw, pitch) = match code {
			"Numpad2" => (0.0, step),
			"Numpad4" => (step, 0.0),
			"Numpad6" => (-step, 0.0),
			"Numpad8" => (0.0, -step),
			"Numpad9" => (std::f32::consts::PI, -2.0 * self.camera.pitch),
			_ => return Ok(false),
		};
		self.cancel_transform(workspace)?;
		self.end_navigation();
		self.click = None;
		self.camera.orbit_angles(Vec2::new(yaw, pitch));
		self.changed();
		Ok(true)
	}

	pub fn keyboard_focused(&self) -> bool {
		self.focused
	}

	pub fn game_keyboard_active(&self) -> bool {
		self.playing && self.settings.view == ViewportView::Game
	}

	fn set_keyboard_focus(
		&mut self,
		focused: bool,
		workspace: &mut EditorWorkspace,
	) -> Result<(), String> {
		if self.focused == focused && (focused || !self.captured()) {
			return Ok(());
		}
		self.focused = focused;
		self.changed();
		if !focused {
			// Internal asset drags belong to the WebView and handle app blur there.
			self.navigation = None;
			self.click = None;
			self.keys.clear();
			workspace.backend.clear_play_input();
			self.cancel_transform(workspace)?;
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

	fn end_navigation(&mut self) {
		if self.navigation.take().is_some() {
			self.keys.clear();
			self.changed();
		}
	}

	pub fn is_flying(&self) -> bool {
		self.navigation
			.as_ref()
			.is_some_and(|nav| nav.mode == NavigationMode::Fly)
	}

	fn persistent_fly(&self) -> bool {
		self.navigation
			.as_ref()
			.is_some_and(|nav| nav.walk_origin.is_some())
	}

	fn restore_walk_camera(&mut self) {
		if let Some(original) = self
			.navigation
			.as_mut()
			.and_then(|nav| nav.walk_origin.take())
		{
			self.camera = original;
		}
	}

	pub fn look_from_fly_motion(&mut self, delta: [f64; 2]) {
		if self.is_flying() && delta != [0.0; 2] {
			// Device motion is relative, independent of the cursor reaching an edge.
			self.camera
				.look(Vec2::new(delta[0] as f32, delta[1] as f32) * 0.35);
			self.changed();
		}
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
		self.is_flying() && self.fly_movement() != Vec3::ZERO
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

	pub fn clear_play_input(&mut self, workspace: &mut EditorWorkspace) {
		self.keys.clear();
		workspace.backend.clear_play_input();
	}

	fn route_game_input(&mut self, workspace: &mut EditorWorkspace) -> Result<(), String> {
		workspace
			.backend
			.set_game_keyboard_input_from_codes(&self.keys)
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
		if self.settings.colliders {
			self.collider_visuals.append(&mut visuals, dpi);
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
