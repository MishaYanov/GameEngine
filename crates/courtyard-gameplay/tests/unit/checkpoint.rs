use super::*;
use crate::ProgressAction as CampaignAction;
use engine_core::{game_progress::ProgressAction, game_session::GameSession};
use serde_json::json;
use std::sync::Arc;

mod checkpoint_compatibility;

struct ProfileDir(PathBuf);

impl ProfileDir {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = std::env::temp_dir().join(format!(
			"engine-courtyard-checkpoint-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}

impl Drop for ProfileDir {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn fixture() -> (ProjectData, SceneData, CheckpointRegistry) {
	let mut project =
		ProjectData::from_json(include_str!("../../../../apps/game/project.json")).unwrap();
	let authored = SceneData::from_json(include_str!(
		"../../../../apps/game/assets/scenes/courtyard.json"
	))
		.unwrap();
	let player = authored
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-player")
		.unwrap();
	let settings = project.gameplay_settings.as_mut().unwrap();
	settings["project_save_id"] = json!("85792c93-fbc9-4aa2-9d67-a51168cf9b76");
	settings["checkpoints"] = json!([{
		"id": "entry",
		"scene_id": "courtyard",
		"player": "courtyard-player",
		"spawn": player.transform.position,
		"activation_radius": 0.25,
	}]);
	let registry = CheckpointRegistry::from_project(&project).unwrap().unwrap();
	(project, authored, registry)
}

#[test]
fn a_local_slot_round_trips_replaces_and_rejects_changed_content() {
	let (project, authored, registry) = fixture();
	let profile = ProfileDir::new();
	assert!(registry.load(&profile.0).unwrap().is_none());
	for score in [1, 3] {
		let progress = CampaignProgress {
			has_courtyard_key: true,
			score,
			..CampaignProgress::default()
		};
		let selected = registry
			.active(&project.startup_scene_id, &authored, &progress)
			.unwrap();
		assert_eq!(selected.id, "entry");
		registry
			.save(&profile.0, selected, &project, &authored, &progress)
			.unwrap();
		let saved = registry.load(&profile.0).unwrap().unwrap();
		let (restored, restored_progress) =
			registry.reconstruct(&saved, &project, &authored).unwrap();
		assert_eq!(restored_progress.score, score);
		assert!(restored_progress.has_courtyard_key);
		assert!(!restored_progress.completed);
		let key = &authored.bindings["key"];
		let visual = restored
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.unwrap();
		assert!(!visual.renderable && !visual.collision_enabled);
		let mut changed = authored.clone();
		changed.entities[0].name.push_str(" revised");
		assert!(
			registry
				.reconstruct(&saved, &project, &changed)
				.err()
				.unwrap()
				.contains("incompatible")
		);
	}
	let parent = registry.slot_parent(&profile.0, false).unwrap().unwrap();
	fs::write(parent.join("slot.json"), vec![b' '; MAX_SAVE_BYTES + 1]).unwrap();
	assert!(
		registry
			.load(&profile.0)
			.err()
			.unwrap()
			.contains("size limit")
	);
}

#[test]
fn checkpoint_config_rejects_overlapping_areas_and_missing_stable_identity() {
	let (mut project, _, _) = fixture();
	let settings = project.gameplay_settings.as_mut().unwrap();
	let mut duplicate = settings["checkpoints"][0].clone();
	duplicate["id"] = json!("second");
	settings["checkpoints"]
		.as_array_mut()
		.unwrap()
		.push(duplicate);
	assert!(
		CheckpointRegistry::from_project(&project)
			.err()
			.unwrap()
			.contains("overlap")
	);
	let settings = project.gameplay_settings.as_mut().unwrap();
	settings.as_object_mut().unwrap().remove("project_save_id");
	assert!(
		CheckpointRegistry::from_project(&project)
			.err()
			.unwrap()
			.contains("project_save_id")
	);
}

#[test]
fn prepared_restore_is_rejected_after_progress_changes_and_commits_a_new_run() {
	let (project, scene, _) = fixture();
	let project = Arc::new(project);
	let scene = Arc::new(scene);
	let scene_id = project.startup_scene_id.clone();
	let factory = crate::resolve(&project).unwrap();
	let mut run = GameSession::with_factory_for_scene(
		Arc::clone(&project),
		scene_id.clone(),
		Arc::clone(&scene),
		factory,
	)
		.unwrap();
	let original_run = run.run_id().to_owned();
	let progress = CampaignProgress {
		score: 3,
		..CampaignProgress::default()
	};
	let stale = run
		.prepare_restored_game(scene_id.clone(), Arc::clone(&scene), progress.clone())
		.unwrap();
	let award =
		ProgressAction::new::<CampaignProgress>(CampaignAction::AwardScore { amount: 1 }).unwrap();
	run.apply_progress_actions(&[award]).unwrap();
	assert!(run.commit_scene(stale).unwrap_err().contains("advanced"));
	assert_eq!(run.run_id(), original_run);
	assert_eq!(run.progress::<CampaignProgress>().unwrap().score, 1);
	let current = run
		.prepare_restored_game(scene_id, scene, progress)
		.unwrap();
	run.commit_scene(current).unwrap();
	assert_ne!(run.run_id(), original_run);
	assert_eq!(run.active().tick_index(), 0);
	assert_eq!(run.progress_revision(), 0);
	assert_eq!(run.progress::<CampaignProgress>().unwrap().score, 3);
}
