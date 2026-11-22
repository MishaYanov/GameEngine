//! Guarded Courtyard campaign edits over retained authored documents.

use std::{collections::HashSet, sync::Arc};

use bridge::editor::{
	ColliderUpdateDto, EditorCommandDto, EditorCommandRequest, EditorSnapshotDto,
	MAX_COLLIDER_UPDATES,
};
use courtyard_gameplay::{
	authoring_diagnostics::{AuthoredScenePreparation, CampaignDiagnostic},
	authoring_repairs::{
		distinct_sensor_repairs, sensor_repairs, CampaignRepairBatch, CampaignRepairExpectation,
		CampaignRepairPreview, CampaignRepairTarget,
	},
	settings::{CampaignDefinition, InteractionSettings, SceneInteractions},
};
use runtime::{project::ProjectData, scene_catalog::SceneId, scene_data::ColliderData, SceneData};
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
	project: Arc<ProjectData>,
	scenes: Vec<(SceneId, Arc<SceneData>)>,
}

impl CampaignEdit {
	/// Prepare the same CPU gameplay configuration as compiler preflight, off the editor lock.
	pub(crate) fn review(self) -> Result<ReviewedCampaignEdit, CampaignAuthoringError> {
		let preparation = AuthoredScenePreparation::new(Arc::clone(&self.project))?;
		for (id, document) in self.scenes {
			preparation
				.prepare(&id, document)
				.map_err(|message| CampaignAuthoringError::scene(id, message))?;
		}
		drop(preparation);
		Ok(ReviewedCampaignEdit {
			expected_revision: self.expected_revision,
			expected_generation: self.expected_generation,
			project: Arc::unwrap_or_clone(self.project),
		})
	}

	fn diagnose(&self, active_scene: &SceneId) -> Vec<CampaignDiagnostic> {
		let preparation = match AuthoredScenePreparation::new(Arc::clone(&self.project)) {
			Ok(preparation) => preparation,
			Err(message) => return vec![CampaignDiagnostic::new(None, message)],
		};
		// The captured route has at most eight chapters and one Finish.
		self.scenes
			.iter()
			.filter_map(|(id, document)| {
				let message = preparation.prepare(id, Arc::clone(document)).err()?;
				let mut issue = CampaignDiagnostic::new(Some(id.clone()), message);
				if id == active_scene {
					let repairs = distinct_sensor_repairs(&self.project, id, document);
					if repairs_pass_preparation(&preparation, id, document, &repairs) {
						if let [repair] = repairs.as_slice() {
							issue.repair = Some(repair.target.clone());
						} else {
							issue.repair_batch =
								repairs.into_iter().map(|repair| repair.target).collect();
						}
					}
				}
				Some(issue)
			})
			.collect()
	}
}

fn repairs_pass_preparation(
	preparation: &AuthoredScenePreparation,
	scene: &SceneId,
	document: &SceneData,
	repairs: &[CampaignRepairPreview],
) -> bool {
	if repairs.is_empty() || repairs.len() > MAX_COLLIDER_UPDATES {
		return false;
	}
	let mut trial = document.clone();
	for repair in repairs {
		let Some(entity) = trial
			.entities
			.iter_mut()
			.find(|entity| entity.key == repair.target.entity_key)
		else {
			return false;
		};
		entity.collider = Some(repair.after.clone());
	}
	preparation.prepare(scene, Arc::new(trial)).is_ok()
}

pub(crate) struct ReviewedCampaignEdit {
	expected_revision: u64,
	expected_generation: u64,
	project: ProjectData,
}

pub(crate) struct CampaignCheck {
	workspace_generation: u64,
	revision: u64,
	active_scene: SceneId,
	draft: Result<CampaignEdit, CampaignAuthoringError>,
}

impl CampaignCheck {
	pub(crate) fn diagnose(self) -> CampaignDiagnosticReport {
		let issues = match self.draft {
			Ok(edit) => edit.diagnose(&self.active_scene),
			Err(error) => vec![CampaignDiagnostic::new(error.scene_id, error.message)],
		};
		CampaignDiagnosticReport {
			workspace_generation: self.workspace_generation,
			revision: self.revision,
			issues,
		}
	}
}

#[derive(Debug, Serialize)]
pub struct CampaignDiagnosticReport {
	pub workspace_generation: u64,
	pub revision: u64,
	pub issues: Vec<CampaignDiagnostic>,
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
		self.capture_campaign_draft(generation, revision, campaign)
	}

	fn capture_campaign_draft(
		&self,
		generation: u64,
		revision: u64,
		campaign: CampaignDefinition,
	) -> Result<CampaignEdit, CampaignAuthoringError> {
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
			project: Arc::new(project),
			scenes,
		})
	}

	pub(crate) fn capture_campaign_check(
		&self,
		generation: u64,
		revision: u64,
		campaign: CampaignDefinition,
	) -> Result<CampaignCheck, CampaignAuthoringError> {
		self.check_campaign_owner(generation, revision)?;
		let draft = self.capture_campaign_draft(generation, revision, campaign);
		Ok(CampaignCheck {
			workspace_generation: generation,
			revision,
			active_scene: self.backend.session().active_scene_id().clone(),
			draft,
		})
	}

	pub(crate) fn accept_campaign_diagnostics(
		&self,
		report: CampaignDiagnosticReport,
	) -> Result<CampaignDiagnosticReport, CampaignAuthoringError> {
		self.check_campaign_owner(report.workspace_generation, report.revision)?;
		Ok(report)
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

	pub(crate) fn preview_campaign_repair(
		&self,
		generation: u64,
		revision: u64,
		target: &CampaignRepairTarget,
	) -> Result<CampaignRepairPreview, CampaignAuthoringError> {
		self.check_campaign_owner(generation, revision)?;
		let session = self.backend.session();
		if session.active_scene_id() != &target.scene_id {
			return Err(CampaignAuthoringError::conflict(
				"Open the repair's scene and run Check Campaign again.".into(),
			));
		}
		let document = session.scene_document(&target.scene_id)?;
		let preview = sensor_repairs(session.project(), &target.scene_id, &document)
			.find(|repair| repair.target == *target);
		preview.ok_or_else(|| {
			CampaignAuthoringError::conflict(
				"This trigger is no longer eligible. Run Check Campaign again.".into(),
			)
		})
	}

	pub(crate) fn apply_campaign_repair(
		&mut self,
		generation: u64,
		revision: u64,
		target: &CampaignRepairTarget,
		expected_collider: &ColliderData,
	) -> Result<EditorSnapshotDto, CampaignAuthoringError> {
		let preview = self.preview_campaign_repair(generation, revision, target)?;
		if &preview.before != expected_collider {
			return Err(CampaignAuthoringError::conflict(
				"The trigger collider changed. Review the repair again.".into(),
			));
		}
		self.execute(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::SetCollider {
				key: target.entity_key.as_str().to_owned(),
				collider: Some(preview.after),
			},
		})
		.map_err(Into::into)
	}

	pub(crate) fn preview_campaign_repair_batch(
		&self,
		generation: u64,
		revision: u64,
		scene: &SceneId,
		targets: &[CampaignRepairTarget],
	) -> Result<CampaignRepairBatch, CampaignAuthoringError> {
		self.check_campaign_owner(generation, revision)?;
		let session = self.backend.session();
		if session.active_scene_id() != scene
			|| !(2..=MAX_COLLIDER_UPDATES).contains(&targets.len())
		{
			return Err(CampaignAuthoringError::conflict(
				"Review the repair batch in its active scene.".into(),
			));
		}
		let document = session.scene_document(scene)?;
		let repairs = distinct_sensor_repairs(session.project(), scene, &document);
		if !repairs.iter().map(|repair| &repair.target).eq(targets) {
			return Err(CampaignAuthoringError::conflict(
				"The eligible trigger batch changed. Run Check Campaign again.".into(),
			));
		}
		Ok(CampaignRepairBatch {
			scene_id: scene.clone(),
			workspace_generation: generation,
			revision,
			repairs,
		})
	}

	pub(crate) fn apply_campaign_repair_batch(
		&mut self,
		generation: u64,
		revision: u64,
		scene: &SceneId,
		expected: &[CampaignRepairExpectation],
	) -> Result<EditorSnapshotDto, CampaignAuthoringError> {
		if !(2..=MAX_COLLIDER_UPDATES).contains(&expected.len()) {
			return Err("Expected two or three reviewed colliders".to_owned().into());
		}
		let targets: Vec<_> = expected.iter().map(|row| row.target.clone()).collect();
		let batch = self.preview_campaign_repair_batch(generation, revision, scene, &targets)?;
		if batch
			.repairs
			.iter()
			.zip(expected)
			.any(|(current, reviewed)| current.before != reviewed.before)
		{
			return Err(CampaignAuthoringError::conflict(
				"A trigger collider changed. Review the repair batch again.".into(),
			));
		}
		let updates = batch
			.repairs
			.into_iter()
			.map(|repair| ColliderUpdateDto {
				key: repair.target.entity_key.as_str().to_owned(),
				collider: repair.after,
			})
			.collect();
		self.execute(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::SetColliders { updates },
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
