//! Edit-only camera preview. The frozen rig never changes scene history or advances Play.
use crate::editor::EditorWorkspace;
use runtime::{
	asset_catalog::AssetId, camera_input::CameraIntent, required_probe_radius,
	scene_catalog::SceneId, scene_data::CameraData, Camera, EntityKey, FrozenOrbitPreview,
};
use serde::Serialize;
use std::sync::Arc;

pub(crate) struct CameraPreview {
	token: String,
	generation: u64,
	scene_id: SceneId,
	revision: u64,
	camera_key: Option<EntityKey>,
	definition_signature: String,
	required_radius: f32,
	state: CameraPreviewState,
}

#[derive(Clone)]
enum CameraPreviewState {
	Fixed(Camera),
	Follow(Camera),
	Orbit(FrozenOrbitPreview),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPreviewMode {
	Fixed,
	Follow,
	Orbit,
}

#[derive(Debug, Serialize)]
pub struct CameraPreviewDto {
	pub token: String,
	pub workspace_generation: u64,
	pub scene_id: SceneId,
	pub revision: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub camera_key: Option<EntityKey>,
	pub mode: CameraPreviewMode,
	pub sample: u64,
	pub required_probe_radius: f32,
	pub effective_distance: f32,
	pub target_lost: bool,
	pub blocker: Option<EntityKey>,
}

impl CameraPreviewState {
	fn mode(&self) -> CameraPreviewMode {
		match self {
			Self::Fixed(_) => CameraPreviewMode::Fixed,
			Self::Follow(_) => CameraPreviewMode::Follow,
			Self::Orbit(_) => CameraPreviewMode::Orbit,
		}
	}
	fn pose(&self) -> &Camera {
		match self {
			Self::Fixed(camera) | Self::Follow(camera) => camera,
			Self::Orbit(preview) => preview.pose(),
		}
	}
	fn orbit(&self) -> Option<&FrozenOrbitPreview> {
		let Self::Orbit(preview) = self else {
			return None;
		};
		Some(preview)
	}
}

impl CameraPreview {
	fn is_current(&self, workspace: &EditorWorkspace) -> bool {
		let session = workspace.backend.session();
		self.generation == workspace.generation
			&& self.revision == session.revision()
			&& &self.scene_id == session.active_scene_id()
			&& !session.is_playing()
			&& !session.has_transaction()
	}

	fn dto(&self) -> CameraPreviewDto {
		let orbit = self.state.orbit().map(FrozenOrbitPreview::orbit_state);
		CameraPreviewDto {
			token: self.token.clone(),
			workspace_generation: self.generation,
			scene_id: self.scene_id.clone(),
			revision: self.revision,
			camera_key: self.camera_key.clone(),
			mode: self.state.mode(),
			sample: orbit.map_or(0, |orbit| orbit.tick),
			required_probe_radius: self.required_radius,
			effective_distance: orbit.map_or(0.0, |orbit| orbit.effective_distance),
			target_lost: orbit.is_some_and(|orbit| orbit.target_lost),
			blocker: orbit
				.and_then(|orbit| orbit.obstruction.as_ref())
				.map(|hit| hit.blocker.clone()),
		}
	}
}

impl EditorWorkspace {
	pub fn open_camera_preview(
		&mut self,
		camera: CameraData,
		expected_revision: u64,
		expected_generation: u64,
		scene_id: &str,
	) -> Result<CameraPreviewDto, String> {
		self.check_camera_preview_context(expected_revision, expected_generation, scene_id)?;
		if camera.orbit.is_none() {
			return Err("Select Orbit before previewing a camera draft".into());
		}
		self.open_camera_definition(camera, None)
	}

	pub fn open_selected_camera_preview(
		&mut self,
		key: EntityKey,
		expected_revision: u64,
		expected_generation: u64,
		scene_id: &str,
	) -> Result<CameraPreviewDto, String> {
		self.check_camera_preview_context(expected_revision, expected_generation, scene_id)?;
		let session = self.backend.session();
		if session.selection() != std::slice::from_ref(&key) {
			return Err("Select the Camera to preview in the current Scene".into());
		}
		let document = session.authored_document();
		let camera = document.object_camera(&key, &document.world_matrices()?)?;
		self.open_camera_definition(camera, Some(key))
	}

	fn check_camera_preview_context(
		&self,
		expected_revision: u64,
		expected_generation: u64,
		scene_id: &str,
	) -> Result<(), String> {
		let session = self.backend.session();
		if self.generation != expected_generation
			|| session.revision() != expected_revision
			|| session.active_scene_id().as_str() != scene_id
		{
			return Err("Camera preview context changed. Reopen it for the current scene.".into());
		}
		if session.is_playing() || session.has_transaction() {
			return Err("Stop Play and finish the current edit before previewing a camera".into());
		}
		if self.prop_preview_active() {
			return Err("Close the Prop preview before previewing a Camera".into());
		}
		Ok(())
	}

	fn open_camera_definition(
		&mut self,
		camera: CameraData,
		camera_key: Option<EntityKey>,
	) -> Result<CameraPreviewDto, String> {
		let session = self.backend.session();
		camera.validate()?;
		let definition_signature =
			serde_json::to_string(&camera).map_err(|error| error.to_string())?;
		if definition_signature.len() > 16 * 1024 {
			return Err("Camera preview definition exceeds 16 KiB".into());
		}
		let required_radius = if camera.orbit.is_some() {
			required_probe_radius(camera.near_plane, camera.vertical_fov)?
		} else {
			0.0
		};
		// An identical, untouched preview can share its frozen geometry with its replacement.
		// A reopened preview always starts at sample zero with a fresh token.
		let state = if let Some(previous) = self.camera_preview.as_ref().filter(|p| {
			p.is_current(self)
				&& p.state
					.orbit()
					.is_none_or(|orbit| orbit.orbit_state().tick == 0)
				&& p.camera_key == camera_key
				&& p.definition_signature == definition_signature
		}) {
			previous.state.clone()
		} else if let Some(settings) = camera.orbit.clone() {
			CameraPreviewState::Orbit(FrozenOrbitPreview::from_document(
				session.authored_document(),
				settings,
				camera.to_camera(),
				1.0 / session.project().fixed_hz as f32,
			)?)
		} else if camera.follow.is_some() {
			let mut document = session.authored_document();
			if document.default_game_camera != camera_key {
				Arc::make_mut(&mut document)
					.default_game_camera
					.clone_from(&camera_key);
			}
			let world = runtime::world::RuntimeWorld::new(document)?;
			let controller = runtime::GameCameraController::new(&world)?;
			CameraPreviewState::Follow(controller.camera().clone())
		} else {
			CameraPreviewState::Fixed(camera.to_camera())
		};
		let preview = CameraPreview {
			token: AssetId::generate().as_str().to_owned(),
			generation: self.generation,
			scene_id: session.active_scene_id().clone(),
			revision: session.revision(),
			camera_key,
			definition_signature,
			required_radius,
			state,
		};
		let reply = preview.dto();
		self.camera_preview = Some(preview);
		Ok(reply)
	}

	pub fn sample_camera_preview(
		&mut self,
		token: &str,
		next_sample: u64,
		intent: CameraIntent,
	) -> Result<CameraPreviewDto, String> {
		self.discard_stale_camera_preview();
		let preview = self
			.camera_preview
			.as_mut()
			.ok_or("Camera preview has closed. Reopen it for the current scene.")?;
		if token != preview.token || next_sample > 9_007_199_254_740_991 {
			return Err("Camera preview token or sample order is stale".into());
		}
		let CameraPreviewState::Orbit(ref mut orbit) = preview.state else {
			return Err("Only Orbit Camera previews accept camera input".into());
		};
		if Some(next_sample) != orbit.orbit_state().tick.checked_add(1) {
			return Err("Camera preview token or sample order is stale".into());
		}
		*orbit = orbit.advance(intent)?;
		Ok(preview.dto())
	}

	pub fn close_camera_preview(&mut self, token: &str) -> bool {
		if self
			.camera_preview
			.as_ref()
			.is_some_and(|p| p.token == token)
		{
			self.camera_preview = None;
			return true;
		}
		false
	}

	pub fn active_camera_preview(&self) -> Option<&Camera> {
		self.camera_preview
			.as_ref()
			.filter(|p| p.is_current(self))
			.map(|p| p.state.pose())
	}

	/// Saving can change the revision without changing any geometry. Tell the host to
	/// repaint if this operation retired a preview, including when the operation cleared it itself.
	pub(crate) fn with_camera_preview_cleanup<T>(
		&mut self,
		operation: impl FnOnce(&mut Self) -> T,
	) -> (T, bool) {
		let had_preview = self.camera_preview.is_some();
		let result = operation(self);
		self.discard_stale_camera_preview();
		(result, had_preview && self.camera_preview.is_none())
	}

	pub(crate) fn discard_stale_camera_preview(&mut self) {
		if self
			.camera_preview
			.as_ref()
			.is_some_and(|p| !p.is_current(self))
		{
			self.camera_preview = None;
		}
	}
}
