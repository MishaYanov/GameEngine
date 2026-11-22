//! CPU preparation and bounded diagnostics for authored scenes.

use std::sync::Arc;

use engine_core::{
	game_session::GameSession, project::ProjectData, scene_catalog::SceneId, scene_data::SceneData,
	simulation_session::GameModuleFactory,
};
use serde::Serialize;

const MAX_ISSUE_BYTES: usize = 1024;

/// Preparation failure associated with a scene or the campaign draft as a whole.
#[derive(Debug, Serialize)]
pub struct CampaignDiagnostic {
	pub scene_id: Option<SceneId>,
	pub message: String,
}

impl CampaignDiagnostic {
	/// Retain the scene identity while bounding the inspector's display message.
	pub fn new(scene_id: Option<SceneId>, mut message: String) -> Self {
		if message.len() > MAX_ISSUE_BYTES {
			let mut end = MAX_ISSUE_BYTES - 3;
			while !message.is_char_boundary(end) {
				end -= 1;
			}
			message.truncate(end);
			message.push_str("...");
		}
		Self { scene_id, message }
	}
}

/// One resolved gameplay factory for a pass over an immutable authored project.
pub struct AuthoredScenePreparation {
	project: Arc<ProjectData>,
	factory: Arc<dyn GameModuleFactory>,
}

impl AuthoredScenePreparation {
	pub fn new(project: Arc<ProjectData>) -> Result<Self, String> {
		let factory = super::resolve(&project)?;
		Ok(Self { project, factory })
	}

	/// Prepare a fresh CPU session with the same role checks used by Play.
	pub fn prepare(
		&self,
		scene_id: &SceneId,
		document: Arc<SceneData>,
	) -> Result<GameSession, String> {
		GameSession::with_factory_for_scene(
			Arc::clone(&self.project),
			scene_id.clone(),
			document,
			Arc::clone(&self.factory),
		)
	}
}
