use super::*;

fn request(generation: u64, deadline: Instant) -> ActionRequest {
	let (reply, _) = mpsc::sync_channel(1);
	ActionRequest {
		action: ViewportActionDto::SetGrid { visible: true },
		workspace_generation: generation,
		reply,
		deadline,
	}
}

#[test]
fn queued_action_rejects_a_replaced_workspace() {
	let now = Instant::now();
	let queued = request(41, now + Duration::from_secs(8));
	assert!(queued.validate_context(41, now).is_ok());
	assert!(queued
		.validate_context(42, now)
		.unwrap_err()
		.contains("different open project"));
}

#[test]
fn action_expiry_is_rechecked_after_waiting_for_the_workspace() {
	let now = Instant::now();
	let queued = request(41, now + Duration::from_secs(8));
	assert!(queued
		.validate_context(41, now + Duration::from_secs(7))
		.is_ok());
	assert!(queued
		.validate_context(41, now + Duration::from_secs(9))
		.unwrap_err()
		.contains("expired"));
}

#[test]
fn restart_rechecks_deadline_revision_run_and_stop_after_preparation() {
	use bridge::editor::{EditorCommandDto, EditorCommandRequest};
	let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game/assets");
	let mut workspace =
		EditorWorkspace::open(assets.to_str().unwrap(), "scenes/courtyard.json").unwrap();
	let execute = |workspace: &mut EditorWorkspace, command| {
		workspace
			.execute_receipt(EditorCommandRequest {
				expected_revision: workspace.backend.session().revision(),
				command,
			})
			.unwrap()
	};
	execute(&mut workspace, EditorCommandDto::StartPlay);
	let status = workspace.backend.play_status().unwrap();
	let revision = workspace.backend.session().revision();
	let now = Instant::now();
	let mut queued = request(workspace.generation, now + Duration::from_secs(8));
	queued.action = ViewportActionDto::RestartRun {
		run_id: status.run_id.clone(),
		expected_revision: revision,
	};
	let guard = PlayInstall::Restart(&queued);
	assert!(guard.validate(&workspace, now).unwrap());
	let (prepared, _) = workspace
		.backend
		.prepare_restart_play(&status.run_id, revision)
		.unwrap();
	assert!(guard
		.validate(&workspace, now + Duration::from_secs(9))
		.unwrap_err()
		.contains("expired"));
	assert_eq!(
		workspace.backend.play_status().unwrap(),
		status,
		"expired staging leaves old run intact"
	);
	workspace.backend.commit_play_scene(prepared).unwrap();
	assert!(
		guard.validate(&workspace, now).is_err(),
		"queued duplicate cannot restart the new run"
	);
	let fresh = workspace.backend.play_status().unwrap();
	queued.action = ViewportActionDto::RestartRun {
		run_id: fresh.run_id.clone(),
		expected_revision: revision,
	};
	let (prepared, _) = workspace
		.backend
		.prepare_restart_play(&fresh.run_id, revision)
		.unwrap();
	execute(&mut workspace, EditorCommandDto::SetPaused { paused: true });
	assert!(PlayInstall::Restart(&queued)
		.validate(&workspace, now)
		.is_err());
	execute(&mut workspace, EditorCommandDto::StopPlay);
	assert!(PlayInstall::Restart(&queued)
		.validate(&workspace, now)
		.is_err());
	assert!(workspace.backend.commit_play_scene(prepared).is_err());
	assert!(workspace.backend.play_status().is_none());
}

#[test]
fn final_editor_publication_wakes_once_then_returns_to_idle() {
	let emitted = Instant::now();
	// A completion immediately after CommitPlacement still needs an
	// event-loop wake even when there is no further pointer/frame work.
	assert_eq!(
		pending_editor_publication(8, 7, emitted),
		Some(emitted + Duration::from_millis(50))
	);
	assert_eq!(
		pending_editor_publication(8, 8, emitted + Duration::from_millis(50)),
		None
	);
}

#[test]
fn a_new_frame_for_the_same_scene_advances_readiness() {
	let scene_id = runtime::scene_catalog::SceneId::new("level-one").unwrap();
	let other = runtime::scene_catalog::SceneId::new("level-two").unwrap();
	let mut status = ViewportStatusDto {
		scene_load: Some(ViewportSceneLoadDto {
			workspace_generation: 41,
			scene_id: scene_id.clone(),
			scene_revision: 7,
			stage: ViewportSceneLoadStage::Presented,
			completed: None,
			total: None,
		}),
		..ViewportStatusDto::default()
	};
	assert!(!advance_presented_revision(
		&mut status,
		42,
		Some(&scene_id),
		9
	));
	assert!(!advance_presented_revision(
		&mut status,
		41,
		Some(&other),
		9
	));
	assert!(!advance_presented_revision(
		&mut status,
		41,
		Some(&scene_id),
		7
	));
	// A hidden viewport can skip an intervening scene and render this
	// scene again at a new revision. The old presented event is stale.
	assert!(advance_presented_revision(
		&mut status,
		41,
		Some(&scene_id),
		9
	));
	assert_eq!(
		serde_json::to_value(status.scene_load).unwrap(),
		serde_json::json!({
			"workspace_generation": 41,
			"scene_id": "level-one",
			"scene_revision": 9,
			"stage": "presented",
		})
	);
}
