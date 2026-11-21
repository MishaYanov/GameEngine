use courtyard_gameplay::{CampaignProgress, ProgressAction, resolve};
use engine_core::{
	SceneData, game_progress::GameProgress, game_session::GameSession, project::ProjectData,
	scene_catalog::SceneId,
};
use serde_json::json;
use std::sync::Arc;

const PROJECT: &str = include_str!("../../../apps/game/project.courtyard-chapters.json");
const GARDEN: &str = include_str!("../../../apps/game/assets/scenes/chapter-garden.json");

#[test]
fn rejects_routes_that_skip_repeat_or_leave_out_a_course() {
	let fixture = ProjectData::from_json(PROJECT).unwrap();
	let mut bad_routes = Vec::new();
	for (field, value) in [
		("/scenes/courtyard/destination_scene", json!("finish")),
		("/scenes/garden/destination_scene", json!("courtyard")),
		("/campaign/chapters/1/id", json!("courtyard")),
		("/campaign/chapters/1/course_scene", json!("courtyard")),
		("/campaign/chapters/1/id", json!("GARDEN")),
		("/campaign/chapters/1/label", json!("  ")),
		("/campaign/finish_scene", json!("garden")),
		("/schema_version", json!(1)),
	] {
		let mut project = fixture.clone();
		*project
			.gameplay_settings
			.as_mut()
			.unwrap()
			.pointer_mut(field)
			.unwrap() = value;
		bad_routes.push(project);
	}
	let mut wrong_start = fixture.clone();
	wrong_start.startup_scene_id = SceneId::new("garden").unwrap();
	bad_routes.push(wrong_start);
	let mut omitted_course = fixture.clone();
	omitted_course.gameplay_settings.as_mut().unwrap()["scenes"]["finish"] = json!({
		"kind": "course", "key_trigger": "finish-key", "exit_trigger": "finish-trigger",
		"destination_scene": "garden",
	});
	bad_routes.push(omitted_course);
	for project in bad_routes {
		assert!(resolve(&project).is_err());
	}
	assert!(resolve(&fixture).is_ok());
	let legacy = ProjectData::from_json(include_str!(
		"../../../apps/game/project.release-learning.json"
	))
		.unwrap();
	assert!(resolve(&legacy).is_ok());
}

#[test]
fn preparation_checks_chapter_roles_and_direct_play_cannot_skip_predecessors() {
	let project = Arc::new(ProjectData::from_json(PROJECT).unwrap());
	let garden = SceneId::new("garden").unwrap();
	let authored = SceneData::from_json(GARDEN).unwrap();
	let mut missing_player = authored.clone();
	missing_player.bindings.remove("player");
	let absent = GameSession::with_factory_for_scene(
		Arc::clone(&project),
		garden.clone(),
		Arc::new(missing_player),
		resolve(&project).unwrap(),
	);
	assert!(absent.err().unwrap().contains("\"player\" binding"));
	let mut direct = GameSession::with_factory_for_scene(
		Arc::clone(&project),
		garden.clone(),
		Arc::new(authored),
		resolve(&project).unwrap(),
	)
		.unwrap();
	direct
		.active_mut()
		.set_controlled_input([-1.0, 0.0, 0.0])
		.unwrap();
	for _ in 0..100 {
		direct.tick(1.0 / 60.0).unwrap();
	}
	let progress = direct.progress::<CampaignProgress>().unwrap();
	assert_eq!(progress.score, 0);
	assert!(!progress.scene_facts(&garden).has_key);
	assert!(direct.pending_scene().is_none());
}

#[test]
fn progress_requires_order_and_scores_each_key_once() {
	let project = ProjectData::from_json(PROJECT).unwrap();
	let seed = resolve(&project).unwrap().create_progress().unwrap();
	let mut progress = seed.view().get::<CampaignProgress>().unwrap().clone();
	let apply = |p: &mut CampaignProgress, action| p.apply(&action);
	assert!(
		apply(
			&mut progress,
			ProgressAction::CollectChapterKey { index: 1 }
		)
			.is_err()
	);
	assert!(
		apply(
			&mut progress,
			ProgressAction::CollectChapterKey { index: usize::MAX }
		)
			.is_err()
	);
	assert!(apply(&mut progress, ProgressAction::CollectCourtyardKey).is_err());
	for _ in 0..2 {
		apply(
			&mut progress,
			ProgressAction::CollectChapterKey { index: 0 },
		)
			.unwrap();
	}
	assert_eq!(progress.score, 1);
	assert!(apply(&mut progress, ProgressAction::ReachChapterExit { index: 0 }).is_err());
	apply(&mut progress, ProgressAction::OpenChapterGate { index: 0 }).unwrap();
	apply(&mut progress, ProgressAction::ReachChapterExit { index: 0 }).unwrap();
	assert!(apply(&mut progress, ProgressAction::CompleteRun).is_err());
	apply(
		&mut progress,
		ProgressAction::CollectChapterKey { index: 1 },
	)
		.unwrap();
	assert!(apply(&mut progress, ProgressAction::OpenChapterGate { index: 1 }).is_err());
	apply(&mut progress, ProgressAction::ReachChapterExit { index: 1 }).unwrap();
	apply(&mut progress, ProgressAction::CompleteRun).unwrap();
	assert!(progress.completed);
	assert_eq!(progress.score, 2);
	progress.validate().unwrap();
}

#[test]
fn a_chapter_score_reserves_room_for_every_remaining_key() {
	let project = ProjectData::from_json(PROJECT).unwrap();
	let seed = resolve(&project).unwrap().create_progress().unwrap();
	let mut progress = seed.view().get::<CampaignProgress>().unwrap().clone();
	progress.score = i32::MAX - 1;
	assert!(progress.validate().unwrap_err().contains("remaining keys"));
	progress.score = i32::MAX - 2;
	progress.validate().unwrap();
	for action in [
		ProgressAction::CollectChapterKey { index: 0 },
		ProgressAction::OpenChapterGate { index: 0 },
		ProgressAction::ReachChapterExit { index: 0 },
		ProgressAction::CollectChapterKey { index: 1 },
	] {
		progress.apply(&action).unwrap();
		progress.validate().unwrap();
	}
	assert_eq!(progress.score, i32::MAX);
	// The original single-course score policy is independent of V2 chapter records.
	assert!(
		CampaignProgress {
			score: i32::MAX,
			..CampaignProgress::default()
		}
			.validate()
			.is_ok()
	);
}
