use super::*;
use crate::{
	animation_preview::timeline::{ClipDraftDto, ClipDraftOperation},
	project_location::ProjectLocation,
};
use bridge::editor::{EditorCommandDto, EditorCommandRequest};
use runtime::animation::TimelineEdit;

fn open_workspace_for_exit() -> EditorWorkspace {
	let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../headless/tests/fixtures/authoring-controller/project.json");
	let location = ProjectLocation::from_manifest(&manifest).unwrap();
	EditorWorkspace::open_project_with_notifier(location, || {}).unwrap()
}

fn open_timeline() -> (EditorWorkspace, ClipDraftDto) {
	let mut workspace = open_workspace_for_exit();
	let asset = workspace.authored_clip_assets().unwrap().remove(0);
	let timeline = workspace
		.open_clip_timeline_draft(&asset.asset_id, workspace.backend.session().revision())
		.unwrap();
	(workspace, timeline)
}

fn add_marker(workspace: &mut EditorWorkspace, timeline: &ClipDraftDto, at: u32) -> ClipDraftDto {
	workspace
		.edit_clip_timeline_draft(
			&timeline.draft_id,
			timeline.draft_revision,
			ClipDraftOperation::Edit {
				edit: TimelineEdit::AddMarker {
					at,
					label: "Impact".into(),
				},
			},
		)
		.unwrap()
}

#[test]
fn exit_review_protects_unsaved_timeline_changes_on_a_clean_scene() {
	let (mut workspace, timeline) = open_timeline();
	let revision = Some(workspace.backend.session().revision());
	assert_eq!(
		ExitReview::capture(Some(&workspace), revision)
			.unwrap()
			.warning,
		None
	);

	let dirty = add_marker(&mut workspace, &timeline, 100);
	assert!(dirty.dirty);
	assert!(!workspace.backend.session().is_dirty());
	assert!(ExitReview::capture(Some(&workspace), revision)
		.unwrap()
		.warning
		.unwrap()
		.contains("Animation Clip Timeline"));

	workspace
		.close_clip_timeline_draft(&timeline.draft_id, true)
		.unwrap();
	assert_eq!(
		ExitReview::capture(Some(&workspace), revision)
			.unwrap()
			.warning,
		None
	);
}

#[test]
fn exit_review_refuses_new_edits_in_an_already_dirty_timeline() {
	let (mut workspace, timeline) = open_timeline();
	let revision = Some(workspace.backend.session().revision());
	let dirty = add_marker(&mut workspace, &timeline, 100);
	let review = ExitReview::capture(Some(&workspace), revision).unwrap();
	review.verify(Some(&workspace)).unwrap();

	let changed = add_marker(&mut workspace, &dirty, 200);
	assert!(changed.dirty);
	assert_eq!(Some(workspace.backend.session().revision()), revision);
	assert_eq!(
		ExitReview::capture(Some(&workspace), revision)
			.unwrap()
			.warning,
		review.warning
	);
	assert!(review.verify(Some(&workspace)).is_err());

	workspace
		.edit_clip_timeline_draft(
			&changed.draft_id,
			changed.draft_revision,
			ClipDraftOperation::Undo,
		)
		.unwrap();
	assert!(review.verify(Some(&workspace)).is_err());
}

#[test]
fn exit_review_refuses_replacement_and_scene_edits_after_review() {
	let mut workspace = open_workspace_for_exit();
	let revision = workspace.backend.session().revision();
	let review = ExitReview::capture(Some(&workspace), Some(revision)).unwrap();
	let replacement = open_workspace_for_exit();
	assert_eq!(replacement.backend.session().revision(), revision);
	assert!(review.verify(Some(&replacement)).is_err());

	workspace
		.execute(EditorCommandRequest {
			expected_revision: revision,
			command: EditorCommandDto::Rename {
				key: "yard-player".into(),
				name: "Reviewed player".into(),
			},
		})
		.unwrap();
	assert!(review.verify(Some(&workspace)).is_err());
}

#[test]
fn exit_review_gate_rejects_a_second_request_until_the_first_finishes() {
	let gate = Mutex::new(());
	let first = exit_review_permit(&gate).unwrap().unwrap();
	assert!(exit_review_permit(&gate).unwrap().is_none());
	drop(first);
	assert!(exit_review_permit(&gate).unwrap().is_some());
}

#[test]
fn window_close_reviews_current_revision_and_file_exit_checks_the_caller_revision() {
	let (workspace, _) = open_timeline();
	let current = workspace.backend.session().revision();
	let close = ExitRequest::WindowClose;
	let stale_file_menu = ExitRequest::FileMenu {
		expected_revision: Some(current + 1),
	};

	assert_eq!(close.reviewed_revision(None), None);
	assert_eq!(close.reviewed_revision(Some(&workspace)), Some(current));
	assert_eq!(
		ExitReview::capture(Some(&workspace), close.reviewed_revision(Some(&workspace)))
			.unwrap()
			.warning,
		None
	);
	assert!(ExitReview::capture(
		Some(&workspace),
		stale_file_menu.reviewed_revision(Some(&workspace))
	)
	.is_err());
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
