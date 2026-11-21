//! Guarded Courtyard campaign edits over retained authored documents.

use std::{collections::HashSet, sync::Arc};

use bridge::editor::{EditorCommandDto, EditorCommandRequest, EditorSnapshotDto};
use courtyard_gameplay::settings::{CampaignDefinition, InteractionSettings, SceneInteractions};
use runtime::{
	game_session::GameSession, project::ProjectData, scene_catalog::SceneId,
	simulation_session::GameModuleFactory, SceneData,
};
use serde::Serialize;

use super::EditorWorkspace;

#[derive(Debug, Serialize)]
pub struct CampaignAuthoringError {
	pub code: &'static str,
	pub message: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub scene_id: Option<SceneId>,
}

impl CampaignAuthoringError {
	fn conflict(message: String) -> Self {
		Self {
			code: "conflict",
			message,
			scene_id: None,
		}
	}

	fn scene(scene: SceneId, reason: String) -> Self {
		Self {
			code: "campaign_validation",
			message: format!("Scene {:?}: {reason}", scene.as_str()),
			scene_id: Some(scene),
		}
	}
}

impl From<String> for CampaignAuthoringError {
	fn from(message: String) -> Self {
		Self {
			code: "campaign_validation",
			message,
			scene_id: None,
		}
	}
}

#[derive(Debug)]
pub(crate) struct CampaignEdit {
	expected_revision: u64,
	expected_generation: u64,
	project: ProjectData,
	scenes: Vec<(SceneId, Arc<SceneData>)>,
}

pub(crate) struct ReviewedCampaignEdit {
	expected_revision: u64,
	expected_generation: u64,
	project: ProjectData,
}

impl CampaignEdit {
	/// Prepare the same CPU gameplay configuration as compiler preflight, off the editor lock.
	pub(crate) fn review(self) -> Result<ReviewedCampaignEdit, CampaignAuthoringError> {
		let project = Arc::new(self.project);
		let factory = courtyard_gameplay::resolve(&project)?;
		for (id, document) in self.scenes {
			review_scene(&project, id, document, &factory)?;
		}
		Ok(ReviewedCampaignEdit {
			expected_revision: self.expected_revision,
			expected_generation: self.expected_generation,
			project: Arc::unwrap_or_clone(project),
		})
	}
}

impl EditorWorkspace {
	pub(crate) fn campaign_generation(&self, revision: u64) -> Result<u64, CampaignAuthoringError> {
		self.check_revision(revision)
			.map_err(CampaignAuthoringError::conflict)?;
		let session = self.backend.session();
		if session.is_playing() || session.has_transaction() {
			return Err(
				"Stop Play and complete grouped edits before changing the campaign."
					.to_owned()
					.into(),
			);
		}
		if session.project().gameplay_module.as_deref()
			!= Some(courtyard_gameplay::COURTYARD_RUN_ID)
		{
			return Err("Campaign settings require a Courtyard Run project."
				.to_owned()
				.into());
		}
		Ok(self.generation)
	}

	pub(crate) fn capture_campaign(
		&self,
		generation: u64,
		revision: u64,
		campaign: CampaignDefinition,
	) -> Result<CampaignEdit, CampaignAuthoringError> {
		self.check_campaign_owner(generation, revision)?;
		let session = self.backend.session();
		let mut project = session.project().clone();
		let route = prepare_campaign(&mut project, &campaign)?;
		let scenes = route
			.into_iter()
			.map(|scene| {
				let document = session
					.scene_document(&scene)
					.map_err(|error| CampaignAuthoringError::scene(scene.clone(), error))?;
				Ok((scene, document))
			})
			.collect::<Result<_, CampaignAuthoringError>>()?;
		Ok(CampaignEdit {
			expected_revision: revision,
			expected_generation: generation,
			project,
			scenes,
		})
	}

	pub(crate) fn commit_campaign(
		&mut self,
		edit: ReviewedCampaignEdit,
	) -> Result<EditorSnapshotDto, CampaignAuthoringError> {
		self.check_campaign_owner(edit.expected_generation, edit.expected_revision)?;
		if self.backend.session().project() == &edit.project {
			return self
				.backend
				.snapshot()
				.map_err(|error| error.to_string().into());
		}
		self.execute(EditorCommandRequest {
			expected_revision: edit.expected_revision,
			command: EditorCommandDto::SetProject {
				project: edit.project,
			},
		})
			.map_err(Into::into)
	}

	fn check_campaign_owner(
		&self,
		generation: u64,
		revision: u64,
	) -> Result<(), CampaignAuthoringError> {
		if self.generation != generation {
			return Err(CampaignAuthoringError::conflict(
				"The workspace changed. Reopen Campaign Settings to review the current project."
					.into(),
			));
		}
		self.campaign_generation(revision)?;
		Ok(())
	}
}

fn prepare_campaign(
	project: &mut ProjectData,
	campaign: &CampaignDefinition,
) -> Result<Vec<SceneId>, CampaignAuthoringError> {
	if !(2..=8).contains(&campaign.chapters.len()) {
		return Err("A Courtyard campaign needs two to eight chapters."
			.to_owned()
			.into());
	}
	let settings = InteractionSettings::from_project(project)?.ok_or_else(|| {
		"Configure Course and Finish roles in Interaction Settings first.".to_owned()
	})?;
	let mut enrolled = HashSet::with_capacity(campaign.chapters.len());
	for chapter in &campaign.chapters {
		let scene = &chapter.course_scene;
		if chapter.id.len() > 32 || chapter.label.len() > 48 {
			return Err(CampaignAuthoringError::scene(
				scene.clone(),
				"Chapter ID or label exceeds the authored byte limit.".into(),
			));
		}
		if !enrolled.insert(scene.as_str()) {
			return Err(CampaignAuthoringError::scene(
				scene.clone(),
				"Choose this Course only once.".into(),
			));
		}
		if !matches!(
			settings.scenes.get(scene.as_str()),
			Some(SceneInteractions::Course { .. })
		) {
			return Err(CampaignAuthoringError::scene(
				scene.clone(),
				"Configure Course interactions first.".into(),
			));
		}
	}
	if !matches!(
		settings.scenes.get(campaign.finish_scene.as_str()),
		Some(SceneInteractions::Finish { .. })
	) {
		return Err(CampaignAuthoringError::scene(
			campaign.finish_scene.clone(),
			"Configure Finish interactions first.".into(),
		));
	}
	for (scene, roles) in &settings.scenes {
		match roles {
			SceneInteractions::Course { .. } if !enrolled.contains(scene.as_str()) => {
				return Err(CampaignAuthoringError::scene(
					SceneId::new(scene)?,
					"Include this configured Course in the chapter order.".into(),
				));
			}
			SceneInteractions::Finish { .. } if scene != campaign.finish_scene.as_str() => {
				return Err(CampaignAuthoringError::scene(
					SceneId::new(scene)?,
					"Remove its Finish role or select it as the only campaign finish.".into(),
				));
			}
			_ => {}
		}
	}

	// Only the authored route changes. Preserve the original serialized
	// representation of independent roles and optional checkpoint fields.
	let payload = project
		.gameplay_settings
		.as_mut()
		.and_then(serde_json::Value::as_object_mut)
		.ok_or_else(|| "Campaign settings are not a project object.".to_owned())?;
	let scene_settings = payload
		.get_mut("scenes")
		.and_then(serde_json::Value::as_object_mut)
		.ok_or_else(|| "Campaign settings have no configured scenes.".to_owned())?;
	for (index, chapter) in campaign.chapters.iter().enumerate() {
		let destination = campaign
			.chapters
			.get(index + 1)
			.map_or(&campaign.finish_scene, |next| &next.course_scene);
		let roles = scene_settings
			.get_mut(chapter.course_scene.as_str())
			.and_then(serde_json::Value::as_object_mut)
			.ok_or_else(|| "The configured Course was not retained.".to_owned())?;
		roles.insert("destination_scene".into(), destination.as_str().into());
	}
	payload.insert("schema_version".into(), 2.into());
	payload.insert(
		"campaign".into(),
		serde_json::to_value(campaign).map_err(|e| e.to_string())?,
	);
	project.startup_scene_id = campaign.chapters[0].course_scene.clone();
	Ok(campaign
		.chapters
		.iter()
		.map(|chapter| chapter.course_scene.clone())
		.chain(std::iter::once(campaign.finish_scene.clone()))
		.collect())
}

fn review_scene(
	project: &Arc<ProjectData>,
	scene: SceneId,
	document: Arc<SceneData>,
	factory: &Arc<dyn GameModuleFactory>,
) -> Result<(), CampaignAuthoringError> {
	GameSession::with_factory_for_scene(
		Arc::clone(project),
		scene.clone(),
		document,
		Arc::clone(factory),
	)
		.map(|_| ())
		.map_err(|reason| CampaignAuthoringError::scene(scene, reason))
}
