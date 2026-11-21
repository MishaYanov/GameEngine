use super::*;

const PRIOR_SLOT: &[u8] = include_bytes!("../fixtures/courtyard-prior-save.json");

fn release_fixture() -> (ProjectData, SceneData, SceneData, CheckpointRegistry) {
	let project = ProjectData::from_json(include_str!(
		"../../../../apps/game/project.release-learning.json"
	))
		.unwrap();
	let updated = SceneData::from_json(include_str!(
		"../../../../apps/game/assets/scenes/courtyard-release-learning.json"
	))
		.unwrap();
	let mut prior = updated.clone();
	let decoration = prior
		.entities
		.iter_mut()
		.find(|entity| entity.key.as_str() == "courtyard-cp-entry")
		.unwrap();
	assert_eq!(decoration.name, "Entry checkpoint floor marker");
	decoration.name = "Entry checkpoint marker".into();
	let registry = CheckpointRegistry::from_project(&project).unwrap().unwrap();
	(project, prior, updated, registry)
}

fn fixture_slot(registry: &CheckpointRegistry, profile: &ProfileDir) -> PathBuf {
	let path = registry
		.slot_parent(&profile.0, true)
		.unwrap()
		.unwrap()
		.join("slot.json");
	fs::write(&path, PRIOR_SLOT).unwrap();
	path
}

#[test]
fn reviewed_slot_is_read_only_until_a_deliberate_save() {
	let (project, prior, updated, registry) = release_fixture();
	let profile = ProfileDir::new();
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| panic!(
			"empty slot requested a scene"
		)),
		ContinueAvailability::Empty
	);
	let path = fixture_slot(&registry, &profile);
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| Ok(prior.clone())),
		ContinueAvailability::Current
	);
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| Ok(updated.clone())),
		ContinueAvailability::CompatibleEarlierRelease
	);
	let saved = registry.load(&profile.0).unwrap().unwrap();
	let (live, progress, status) = registry
		.reconstruct_with_status(&saved, &project, &updated)
		.unwrap();
	assert_eq!(status, ContinueAvailability::CompatibleEarlierRelease);
	assert!(progress.has_courtyard_key && progress.route_gate_open);
	assert_eq!(progress.score, 1);
	let gate = live
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-route-gate")
		.unwrap();
	assert!(!gate.renderable && !gate.collision_enabled);
	assert_eq!(fs::read(&path).unwrap(), PRIOR_SLOT);

	let checkpoint = registry.active(saved.scene_id(), &live, &progress).unwrap();
	registry
		.save(&profile.0, checkpoint, &project, &updated, &progress)
		.unwrap();
	assert_ne!(fs::read(&path).unwrap(), PRIOR_SLOT);
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| Ok(updated)),
		ContinueAvailability::Current
	);
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| Ok(prior)),
		ContinueAvailability::IncompatibleRelease
	);
}

#[test]
fn unlisted_release_keeps_both_slot_bytes_and_current_run() {
	let (project, _, mut unlisted, registry) = release_fixture();
	unlisted.entities[0].name.push_str(" unrelated edit");
	let profile = ProfileDir::new();
	let slot = fixture_slot(&registry, &profile);
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| Ok(unlisted.clone())),
		ContinueAvailability::IncompatibleRelease
	);
	let saved = registry.load(&profile.0).unwrap().unwrap();
	let project = Arc::new(project);
	let authored = Arc::new(unlisted);
	let run = GameSession::with_factory_for_scene(
		Arc::clone(&project),
		project.startup_scene_id.clone(),
		Arc::clone(&authored),
		crate::resolve(&project).unwrap(),
	)
		.unwrap();
	let run_id = run.run_id().to_owned();
	assert!(
		registry
			.reconstruct(&saved, &project, &authored)
			.err()
			.unwrap()
			.contains("incompatible")
	);
	assert_eq!(run.run_id(), run_id);
	assert_eq!(run.progress::<CampaignProgress>().unwrap().score, 0);
	assert_eq!(fs::read(slot).unwrap(), PRIOR_SLOT);
}

#[test]
fn the_rule_rejects_other_checkpoints_projects_versions_and_invalid_facts() {
	let (project, _, updated, registry) = release_fixture();
	let profile = ProfileDir::new();
	let slot = fixture_slot(&registry, &profile);
	let target = content_digest(&project, &updated).unwrap();
	let mut envelope: SaveEnvelope = serde_json::from_slice(PRIOR_SLOT).unwrap();

	envelope.saved.checkpoint_id = "entry".into();
	assert!(!compatibility::approves_upgrade(&envelope.saved, target));
	assert!(
		registry
			.reconstruct(&envelope.saved, &project, &updated)
			.is_err()
	);
	envelope.saved.checkpoint_id = "after_gate".into();

	envelope.saved.project_save_id = "38c4b6c3-7212-4954-b396-33a2772517df".into();
	assert!(!compatibility::approves_upgrade(&envelope.saved, target));
	assert!(
		registry
			.reconstruct(&envelope.saved, &project, &updated)
			.is_err()
	);
	envelope.saved.project_save_id = registry.project_save_id().into();

	envelope.saved.version += 1;
	assert!(!compatibility::approves_upgrade(&envelope.saved, target));
	assert!(
		registry
			.reconstruct(&envelope.saved, &project, &updated)
			.is_err()
	);
	envelope.checksum = save_checksum(&envelope.saved).unwrap();
	fs::write(&slot, serde_json::to_vec(&envelope).unwrap()).unwrap();
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| panic!(
			"unsupported version requested a scene"
		)),
		ContinueAvailability::IncompatibleRelease
	);
	envelope.saved.version = SAVE_VERSION;

	envelope.saved.facts.has_key = false;
	envelope.checksum = save_checksum(&envelope.saved).unwrap();
	let invalid_facts = serde_json::to_vec(&envelope).unwrap();
	fs::write(&slot, &invalid_facts).unwrap();
	let saved = registry.load(&profile.0).unwrap().unwrap();
	assert!(registry.reconstruct(&saved, &project, &updated).is_err());
	assert_eq!(fs::read(&slot).unwrap(), invalid_facts);

	let mut corrupt = envelope;
	corrupt.checksum[0] ^= 1;
	let corrupt = serde_json::to_vec(&corrupt).unwrap();
	fs::write(&slot, &corrupt).unwrap();
	assert_eq!(
		registry.inspect(&profile.0, &project, |_| panic!(
			"invalid checksum requested a scene"
		)),
		ContinueAvailability::Unreadable
	);
	assert_eq!(fs::read(slot).unwrap(), corrupt);
}
