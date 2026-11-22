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
		fs::write(root.join("project.json"), test_project::manifest("animation-room"))
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

#[path = "../../../../../tests/support/project_fixture.rs"]
mod test_project;
