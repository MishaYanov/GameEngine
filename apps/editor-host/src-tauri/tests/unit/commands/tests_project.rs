use super::*;
use crate::{
	animation_preview::timeline::{ClipDraftDto, ClipDraftOperation},
	project_location::ProjectLocation,
};
use runtime::animation::TimelineEdit;

fn open_timeline() -> (EditorWorkspace, ClipDraftDto) {
	let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../headless/tests/fixtures/authoring-controller/project.json");
	let location = ProjectLocation::from_manifest(&manifest).unwrap();
	let mut workspace = EditorWorkspace::open_project_with_notifier(location, || {}).unwrap();
	let asset = workspace.authored_clip_assets().unwrap().remove(0);
	let timeline = workspace
		.open_clip_timeline_draft(&asset.asset_id, workspace.backend.session().revision())
		.unwrap();
	(workspace, timeline)
}

#[test]
fn exit_review_protects_unsaved_timeline_changes_on_a_clean_scene() {
	let (mut workspace, timeline) = open_timeline();
	let revision = Some(workspace.backend.session().revision());
	assert_eq!(exit_review(Some(&workspace), revision).unwrap(), None);

	let dirty = workspace
		.edit_clip_timeline_draft(
			&timeline.draft_id,
			timeline.draft_revision,
			ClipDraftOperation::Edit {
				edit: TimelineEdit::AddMarker {
					at: 100,
					label: "Impact".into(),
				},
			},
		)
		.unwrap();
	assert!(dirty.dirty);
	assert!(!workspace.backend.session().is_dirty());
	assert!(exit_review(Some(&workspace), revision)
		.unwrap()
		.unwrap()
		.contains("Animation Clip Timeline"));

	workspace
		.close_clip_timeline_draft(&timeline.draft_id, true)
		.unwrap();
	assert_eq!(exit_review(Some(&workspace), revision).unwrap(), None);
}

#[test]
fn invalid_project_request_returns_an_error_without_building() {
	let result = compiler::create_authoring_project("../escape", "relative");
	assert!(matches!(result, Err(message) if message.contains("project name")));
}

#[test]
fn authoring_project_json_matches_the_frontend_contract() {
	let result = ProjectResult {
		path: "D:/Projects/Demo".into(),
		manifest_path: "D:/Projects/Demo/project.json".into(),
	};
	assert_eq!(
		serde_json::to_value(result).unwrap(),
		serde_json::json!({
			"path": "D:/Projects/Demo", "manifestPath": "D:/Projects/Demo/project.json"
		})
	);
}
