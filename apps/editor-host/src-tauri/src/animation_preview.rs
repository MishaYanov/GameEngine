//! One isolated, pinned native pose. It never advances a game session or edits a document.
use crate::editor::EditorWorkspace;
use runtime::{
	animation::{sample_pose, AnimationPose},
	asset_catalog::AssetId,
	EntityKey,
};
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
}
impl AnimationPreview {
	fn context(&self, workspace: &EditorWorkspace) -> bool {
		self.generation == workspace.generation
			&& self.revision == workspace.backend.session().revision()
			&& !workspace.backend.session().is_playing()
			&& !workspace.backend.session().has_transaction()
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
	pub fn active_animation_preview(&self) -> Option<(EntityKey, Arc<AnimationPose>)> {
		self.animation_preview
			.as_ref()
			.filter(|preview| preview.context(self))
			.map(|preview| (preview.entity.clone(), Arc::clone(&preview.pose)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::project_location::ProjectLocation;
	use std::{fs, path::PathBuf};
	struct Fixture(PathBuf);
	impl Fixture {
		fn new() -> Self {
			let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
			let root = repository.join("target").join(format!(
				"qa-animation-preview-{}",
				AssetId::generate().as_str()
			));
			for directory in ["assets/scenes", "assets/characters", "assets/props"] {
				fs::create_dir_all(root.join(directory)).unwrap();
			}
			fs::copy(
				repository.join("apps/game/animation-demo.json"),
				root.join("project.json"),
			)
			.unwrap();
			for file in [
				"asset-catalog.json",
				"scenes/animation-room.json",
				"characters/two-joint.glb",
				"props/cube.glb",
				"props/capsule.glb",
			] {
				fs::copy(
					repository.join("apps/game/assets").join(file),
					root.join("assets").join(file),
				)
				.unwrap();
			}
			Self(root)
		}
		fn workspace(&self) -> EditorWorkspace {
			EditorWorkspace::open_project_with_notifier(
				ProjectLocation::from_manifest(&self.0.join("project.json")).unwrap(),
				|| {},
			)
			.unwrap()
		}
	}
	impl Drop for Fixture {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}
	#[test]
	fn scrub_is_native_isolated_pinned_and_rejects_invalid_time_without_losing_pose() {
		let fixture = Fixture::new();
		let mut workspace = fixture.workspace();
		let before = workspace.backend.session().authored_document();
		let revision = workspace.backend.session().revision();
		let preview = workspace
			.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
			.unwrap();
		let first = workspace.active_animation_preview().unwrap().1;
		workspace
			.sample_animation_preview(&preview.token, 0.5)
			.unwrap();
		let sampled = workspace.active_animation_preview().unwrap().1;
		assert_ne!(first.local, sampled.local);
		assert!(workspace
			.sample_animation_preview(&preview.token, f64::NAN)
			.is_err());
		assert!(Arc::ptr_eq(
			&sampled,
			&workspace.active_animation_preview().unwrap().1
		));
		assert_eq!(workspace.backend.session().authored_document(), before);
		assert_eq!(workspace.backend.session().revision(), revision);
		assert!(!workspace.backend.session().is_playing());
		fs::write(
			fixture.0.join("assets/characters/two-joint.glb"),
			b"changed source",
		)
		.unwrap();
		workspace
			.sample_animation_preview(&preview.token, 0.25)
			.unwrap(); // immutable accepted source remains pinned
		assert!(workspace
			.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
			.is_err());
		workspace.close_animation_preview(&preview.token);
		assert!(workspace.active_animation_preview().is_none());
	}
	#[test]
	fn revision_generation_and_replacement_tokens_invalidate_preview() {
		let fixture = Fixture::new();
		let mut workspace = fixture.workspace();
		let revision = workspace.backend.session().revision();
		let old = workspace
			.open_animation_preview(EntityKey::new("player").unwrap(), 0, revision)
			.unwrap();
		let current = workspace
			.open_animation_preview(EntityKey::new("player").unwrap(), 1, revision)
			.unwrap();
		workspace.close_animation_preview(&old.token);
		assert!(workspace.active_animation_preview().is_some());
		assert!(workspace.sample_animation_preview(&old.token, 0.).is_err());
		workspace.backend.supersede_revision(revision).unwrap();
		assert!(workspace.active_animation_preview().is_none());
		assert!(workspace
			.sample_animation_preview(&current.token, 0.)
			.is_err());
		let next = workspace
			.open_animation_preview(
				EntityKey::new("player").unwrap(),
				0,
				workspace.backend.session().revision(),
			)
			.unwrap();
		workspace.generation += 1;
		assert!(workspace.active_animation_preview().is_none());
		assert!(workspace.sample_animation_preview(&next.token, 0.).is_err());
	}
}
