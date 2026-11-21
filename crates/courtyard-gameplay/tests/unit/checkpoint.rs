use super::*;
use crate::ProgressAction as CampaignAction;
use engine_core::{game_progress::ProgressAction, game_session::GameSession};
use serde_json::json;
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};

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

struct SlotFixture {
	project: ProjectData,
	authored: SceneData,
	registry: CheckpointRegistry,
	profile: ProfileDir,
}

impl SlotFixture {
	fn new() -> Self {
		let (project, authored, registry) = fixture();
		Self {
			project,
			authored,
			registry,
			profile: ProfileDir::new(),
		}
	}

	fn inspect(&self, slot: SaveSlot) -> SlotInspection {
		self.registry
			.inspect_slot(&self.profile.0, slot, &self.project, |_| {
				Ok(self.authored.clone())
			})
	}

	fn save(&self, slot: SaveSlot, score: i32) {
		let expected = self.inspect(slot).generation.unwrap();
		self.save_at(slot, &expected, score).unwrap();
	}

	fn save_at(&self, slot: SaveSlot, expected: &SlotGeneration, score: i32) -> Result<(), String> {
		self.registry.save_slot(
			&self.profile.0,
			slot,
			expected,
			self.request(&Self::progress(score)),
			|_| Ok(self.authored.clone()),
		)
	}

	fn request<'a>(&'a self, progress: &'a CampaignProgress) -> CheckpointWrite<'a> {
		CheckpointWrite {
			checkpoint: self
				.registry
				.active(&self.project.startup_scene_id, &self.authored, progress)
				.unwrap(),
			project: &self.project,
			authored: &self.authored,
			progress,
		}
	}

	fn parent(&self) -> PathBuf {
		self.registry
			.slot_parent(&self.profile.0, false)
			.unwrap()
			.unwrap()
	}

	fn progress(score: i32) -> CampaignProgress {
		CampaignProgress {
			score,
			has_courtyard_key: true,
			..CampaignProgress::default()
		}
	}
}

#[test]
fn menu_snapshot_loads_the_same_authored_scene_once() {
	let fixture = SlotFixture::new();
	fixture.save(SaveSlot::One, 1);
	fixture.save(SaveSlot::Two, 2);
	let mut loads = 0;
	let slots = fixture
		.registry
		.inspect_slots(&fixture.profile.0, &fixture.project, |_| {
			loads += 1;
			Ok(fixture.authored.clone())
		});
	assert_eq!(loads, 1);
	assert_eq!(slots[0].primary, ContinueAvailability::Current);
	assert_eq!(slots[1].primary, ContinueAvailability::Current);
	assert_eq!(slots[2].primary, ContinueAvailability::Empty);
}

#[test]
fn fixed_slots_preserve_the_original_name_and_reject_stale_or_other_slot_tokens() {
	let fixture = SlotFixture::new();
	let empty = fixture.inspect(SaveSlot::Two).generation.unwrap();
	assert!(
		fixture
			.registry
			.slot_parent(&fixture.profile.0, false)
			.unwrap()
			.is_none()
	);
	fixture.save(SaveSlot::One, 1);
	let parent = fixture.parent();
	let legacy = fs::read(parent.join("slot.json")).unwrap();
	fixture.save_at(SaveSlot::Two, &empty, 2).unwrap();
	assert!(
		fixture
			.save_at(SaveSlot::Two, &empty, 3)
			.unwrap_err()
			.contains("changed")
	);
	assert!(
		fixture
			.save_at(SaveSlot::Three, &empty, 3)
			.unwrap_err()
			.contains("changed")
	);
	assert_eq!(
		fixture.inspect(SaveSlot::Three).primary,
		ContinueAvailability::Empty
	);
	assert_eq!(fs::read(parent.join("slot.json")).unwrap(), legacy);
	assert_ne!(fs::read(parent.join("slot-2.json")).unwrap(), legacy);
}

#[test]
fn recovery_is_explicit_and_a_valid_incompatible_primary_keeps_its_place() {
	let mut fixture = SlotFixture::new();
	let slot = SaveSlot::One;
	fixture.save(slot, 1);
	fixture.save(slot, 2);
	let parent = fixture.parent();
	let primary = parent.join(slot.primary());
	let previous = parent.join(slot.previous());
	let expected_previous = fs::read(&previous).unwrap();
	assert!(fixture.inspect(slot).recovery_checkpoint_id.is_none());

	// A content change makes the old primary incompatible. It remains
	// separate from a corruption that permits recovery.
	let original_name = fixture.authored.entities[0].name.clone();
	fixture.authored.entities[0].name.push_str(" revised");
	assert_eq!(
		fixture.inspect(slot).primary,
		ContinueAvailability::IncompatibleRelease
	);
	assert!(fixture.inspect(slot).recovery_checkpoint_id.is_none());
	fixture.authored.entities[0].name = original_name;

	// Future records can add fields. Preserve unsupported versions even
	// when their payload cannot be decoded with today's V1 schema.
	fs::write(&primary, br#"{"saved":{"version":2,"new_field":true}}"#).unwrap();
	assert_eq!(
		fixture.inspect(slot).primary,
		ContinueAvailability::IncompatibleRelease
	);
	assert!(fixture.inspect(slot).recovery_checkpoint_id.is_none());

	let corrupt = b"short interrupted write";
	fs::write(&primary, corrupt).unwrap();
	let status = fixture.inspect(slot);
	assert_eq!(status.primary, ContinueAvailability::Unreadable);
	assert_eq!(status.recovery_checkpoint_id.as_deref(), Some("entry"));
	let expected = status.generation.unwrap();
	assert!(
		fixture
			.registry
			.load_slot_for_restore(&fixture.profile.0, slot, &expected, RestoreSource::Current)
			.is_err()
	);
	let prior = fixture
		.registry
		.load_slot_for_restore(&fixture.profile.0, slot, &expected, RestoreSource::Previous)
		.unwrap();
	let (_, restored) = fixture
		.registry
		.reconstruct(&prior, &fixture.project, &fixture.authored)
		.unwrap();
	assert_eq!(restored.score, 1);
	assert_eq!(fs::read(&primary).unwrap(), corrupt);
	assert_eq!(fs::read(&previous).unwrap(), expected_previous);

	fixture.save(slot, 4);
	assert_eq!(fs::read(previous).unwrap(), expected_previous);
}

#[test]
fn a_publication_failure_keeps_an_existing_compatible_generation() {
	let fixture = SlotFixture::new();
	let slot = SaveSlot::One;
	let fail_save = |fail_on: &str| {
		let expected = fixture.inspect(slot).generation.unwrap();
		fixture.registry.publish_save(
			&fixture.profile.0,
			slot,
			Some(&expected),
			fixture.request(&SlotFixture::progress(3)),
			|_| panic!("same-scene fixture"),
			|parent, name, bytes| {
				if name == fail_on {
					return Err("injected publication failure".into());
				}
				publish_record(parent, name, bytes)
			},
		)
	};
	assert_eq!(
		fail_save(slot.primary()).unwrap_err(),
		"injected publication failure"
	);
	assert_eq!(fixture.inspect(slot).primary, ContinueAvailability::Empty);

	fixture.save(slot, 1);
	fixture.save(slot, 2);
	let parent = fixture.parent();
	let primary = parent.join(slot.primary());
	let previous = parent.join(slot.previous());
	let before = fs::read(&primary).unwrap();
	let before_previous = fs::read(&previous).unwrap();
	assert_eq!(
		fail_save(slot.previous()).unwrap_err(),
		"injected publication failure"
	);
	assert_eq!(fs::read(&primary).unwrap(), before);
	assert_eq!(fs::read(&previous).unwrap(), before_previous);
	assert_eq!(
		fail_save(slot.primary()).unwrap_err(),
		"injected publication failure"
	);
	assert_eq!(fs::read(&primary).unwrap(), before);
	assert_eq!(fs::read(&previous).unwrap(), before);
	assert!(fixture.inspect(slot).primary.can_continue());
}

#[test]
fn a_busy_publication_lock_refuses_the_write_until_released() {
	let fixture = SlotFixture::new();
	let slot = SaveSlot::Two;
	let parent = fixture
		.registry
		.slot_parent(&fixture.profile.0, true)
		.unwrap()
		.unwrap();
	let expected = fixture.inspect(slot).generation.unwrap();
	let held = lock_slot(&parent, slot).unwrap();
	assert!(
		fixture
			.save_at(slot, &expected, 1)
			.unwrap_err()
			.contains("lock")
	);
	assert!(!parent.join(slot.primary()).exists());
	drop(held);
	fixture.save_at(slot, &expected, 1).unwrap();
	assert_eq!(fixture.inspect(slot).primary, ContinueAvailability::Current);
}

#[test]
fn an_uninspectable_previous_generation_blocks_save_but_preserves_current_continue() {
	let fixture = SlotFixture::new();
	let slot = SaveSlot::One;
	fixture.save(slot, 1);
	let parent = fixture.parent();
	let previous = parent.join(slot.previous());
	let oversized = vec![b' '; MAX_SAVE_BYTES + 1];
	fs::write(&previous, &oversized).unwrap();
	let primary = fs::read(parent.join(slot.primary())).unwrap();
	let inspected = fixture.inspect(slot);
	let expected = inspected.generation.unwrap();
	assert_eq!(inspected.primary, ContinueAvailability::Current);
	assert!(!expected.can_save());
	assert!(
		fixture
			.registry
			.load_slot_for_restore(&fixture.profile.0, slot, &expected, RestoreSource::Current,)
			.is_ok()
	);
	assert!(
		fixture
			.save_at(slot, &expected, 2)
			.unwrap_err()
			.contains("Previous Courtyard")
	);
	assert_eq!(fs::read(parent.join(slot.primary())).unwrap(), primary);
	assert_eq!(fs::read(previous).unwrap(), oversized);
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
