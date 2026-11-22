//! One isolated, pinned native pose. It never advances a game session or edits a document.
#[path = "animation_preview/scoped.rs"]
mod scoped;

use crate::editor::EditorWorkspace;
use runtime::{
	animation::{sample_pose, AnimationPose},
	asset_catalog::AssetId,
	EntityKey,
};
pub use scoped::AnimationWorkspaceTarget;
use scoped::ScopedAnimationPreview;
use serde::Serialize;
use std::sync::Arc;

pub(crate) struct AnimationPreview {
	token: String,
	generation: u64,
	revision: u64,
	entity: EntityKey,
	clip: u32,
	placement: glam::Mat4,
	pose: Arc<AnimationPose>,
	isolated: Option<ScopedAnimationPreview>,
}
#[derive(Serialize)]
pub struct AnimationPreviewDto {
	pub token: String,
	pub workspace_generation: u64,
	pub revision: u64,
	pub entity_key: EntityKey,
	pub clip_index: u32,
	pub duration_seconds: f32,
	pub time_seconds: f64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub target: Option<AnimationWorkspaceTarget>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub request_id: Option<u64>,
}
impl AnimationPreview {
	fn context(&self, workspace: &EditorWorkspace) -> bool {
		self.generation == workspace.generation
			&& self.revision == workspace.backend.session().revision()
			&& !workspace.backend.session().is_playing()
			&& !workspace.backend.session().has_transaction()
			&& self
				.isolated
				.as_ref()
				.is_none_or(|scope| scope.context(workspace))
	}
	fn dto(&self, seconds: f64) -> AnimationPreviewDto {
		AnimationPreviewDto {
			token: self.token.clone(),
			workspace_generation: self.generation,
			revision: self.revision,
			entity_key: self.entity.clone(),
			clip_index: self.clip,
			duration_seconds: self.pose.asset.clips[self.clip as usize].duration_seconds,
			time_seconds: seconds,
			target: self.isolated.as_ref().map(|scope| scope.target.clone()),
			request_id: self
				.isolated
				.as_ref()
				.map(|scope| scope.frame.state.request_id),
		}
	}
}
impl EditorWorkspace {
	pub fn open_animation_preview(
		&mut self,
		entity: EntityKey,
		clip: u32,
		expected_revision: u64,
	) -> Result<AnimationPreviewDto, String> {
		let session = self.backend.session();
		if session.revision() != expected_revision
			|| session.is_playing()
			|| session.has_transaction()
		{
			return Err(
				"Stop Play and finish the current edit; reopen animation preview for this revision"
					.into(),
			);
		}
		if self.has_active_animation_aperture() {
			return Err("Use the Animation workspace to preview the selected clip".into());
		}
		let data = session.authored_document();
		let index = data
			.entities
			.iter()
			.position(|item| item.key == entity)
			.ok_or("Preview entity no longer exists")?;
		let component = data.entities[index]
			.animation
			.as_ref()
			.ok_or("Apply an Animation component before previewing clips")?;
		let asset = bridge::animation_asset::load(&self.asset_root, &component.settings.source)?;
		asset.validate_settings(&component.settings)?;
		let placement = data.validated_world_matrices()?[index];
		let pose = Arc::new(sample_pose(asset, clip, 0., placement, 0)?);
		let candidate = AnimationPreview {
			token: AssetId::generate().as_str().to_owned(),
			generation: self.generation,
			revision: expected_revision,
			entity,
			clip,
			placement,
			pose,
			isolated: None,
		};
		let result = candidate.dto(0.);
		self.animation_preview = Some(candidate);
		Ok(result)
	}
	pub fn sample_animation_preview(
		&mut self,
		token: &str,
		seconds: f64,
	) -> Result<AnimationPreviewDto, String> {
		let preview = self
			.animation_preview
			.as_ref()
			.ok_or("Animation preview is closed")?;
		if preview.token != token || !preview.context(self) {
			return Err("Animation preview is stale; reopen it for the current scene".into());
		}
		let revision = preview
			.pose
			.revision
			.checked_add(1)
			.ok_or("Preview revision exhausted")?;
		let pose = Arc::new(sample_pose(
			Arc::clone(&preview.pose.asset),
			preview.clip,
			seconds,
			preview.placement,
			revision,
		)?);
		let preview = self
			.animation_preview
			.as_mut()
			.ok_or("Animation preview is closed")?;
		preview.pose = pose;
		Ok(preview.dto(seconds))
	}
	pub fn close_animation_preview(&mut self, token: &str) {
		if self
			.animation_preview
			.as_ref()
			.is_some_and(|preview| preview.token == token)
		{
			self.animation_preview = None;
		}
	}
	pub(crate) fn discard_stale_animation_preview(&mut self) -> bool {
		let stale = self
			.animation_preview
			.as_ref()
			.is_some_and(|preview| !preview.context(self));
		if stale {
			self.animation_preview = None;
		}
		stale
	}
	pub fn active_animation_preview(&self) -> Option<(EntityKey, Arc<AnimationPose>)> {
		self.animation_preview
			.as_ref()
			.filter(|preview| preview.isolated.is_none() && preview.context(self))
			.map(|preview| (preview.entity.clone(), Arc::clone(&preview.pose)))
	}
}

#[cfg(test)]
#[path = "../tests/unit/animation_preview.rs"]
mod tests;
