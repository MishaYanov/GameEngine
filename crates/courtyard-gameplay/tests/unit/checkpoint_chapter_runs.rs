use super::*;

struct Chapters {
	project: ProjectData,
	registry: CheckpointRegistry,
	profile: ProfileDir,
	garden: SceneData,
	id: SceneId,
}

impl Chapters {
	fn new() -> Self {
		let project = ProjectData::from_json(include_str!(
			"../../../../apps/game/project.courtyard-chapters.json"
		))
			.unwrap();
		Self {
			registry: CheckpointRegistry::from_project(&project).unwrap().unwrap(),
			project,
			profile: ProfileDir::new(),
			garden: SceneData::from_json(include_str!(
				"../../../../apps/game/assets/scenes/chapter-garden.json"
			))
				.unwrap(),
			id: SceneId::new("garden").unwrap(),
		}
	}

	fn garden_progress(&self, key: bool) -> CampaignProgress {
		let initial = crate::resolve(&self.project)
			.unwrap()
			.create_progress()
			.unwrap();
		let mut progress = initial.view().get::<CampaignProgress>().unwrap().clone();
		for action in [
			CampaignAction::CollectChapterKey { index: 0 },
			CampaignAction::OpenChapterGate { index: 0 },
			CampaignAction::ReachChapterExit { index: 0 },
		] {
			progress.apply(&action).unwrap();
		}
		if key {
			progress
				.apply(&CampaignAction::CollectChapterKey { index: 1 })
				.unwrap();
		}
		progress
	}

	fn slot_path(&self, name: &str) -> PathBuf {
		self.registry
			.slot_parent(&self.profile.0, true)
			.unwrap()
			.unwrap()
			.join(name)
	}

	fn inspect(&self, registry: &CheckpointRegistry, project: &ProjectData) -> SlotInspection {
		registry.inspect_slot(&self.profile.0, SaveSlot::Two, project, |id| {
			assert_eq!(id, &self.id);
			Ok(self.garden.clone())
		})
	}

	fn save(&self, key: bool) {
		let progress = self.garden_progress(key);
		let checkpoint = self
			.registry
			.active(&self.id, &self.garden, &progress)
			.unwrap();
		assert_eq!(checkpoint.id, "garden_entry");
		let generation = self
			.inspect(&self.registry, &self.project)
			.generation
			.unwrap();
		self.registry
			.save_slot(
				&self.profile.0,
				SaveSlot::Two,
				&generation,
				CheckpointWrite {
					checkpoint,
					project: &self.project,
					authored: &self.garden,
					progress: &progress,
				},
				|_| Ok(self.garden.clone()),
			)
			.unwrap();
	}
}

#[test]
fn garden_v2_restores_local_key_and_the_earlier_exited_course() {
	let chapters = Chapters::new();
	chapters.save(true);
	let inspection = chapters.inspect(&chapters.registry, &chapters.project);
	assert_eq!(inspection.primary, ContinueAvailability::Current);
	assert_eq!(inspection.chapter_label.as_deref(), Some("Garden"));
	let file = chapters.slot_path("slot-2.json");
	let bytes = fs::read(&file).unwrap();
	let record: SaveEnvelope = serde_json::from_slice(&bytes).unwrap();
	assert_eq!(record.saved.version, CHAPTER_SAVE_VERSION);
	assert_eq!(record.saved.chapter.as_ref().unwrap().id, "garden");
	let (scene, restored) = chapters
		.registry
		.reconstruct(&record.saved, &chapters.project, &chapters.garden)
		.unwrap();
	let earlier = restored.scene_facts(&chapters.project.startup_scene_id);
	let current = restored.scene_facts(&chapters.id);
	assert!(earlier.has_key && earlier.gate_open && earlier.exited);
	assert!(current.has_key && !current.gate_open && !current.exited);
	assert_eq!(restored.score, 2);
	let key = scene
		.entities
		.iter()
		.find(|entity| entity.key == scene.bindings["key"])
		.unwrap();
	assert!(!key.renderable && !key.collision_enabled);
	assert_eq!(fs::read(file).unwrap(), bytes);
}

#[test]
fn changed_course_document_or_chapter_order_retains_an_incompatible_primary() {
	let chapters = Chapters::new();
	chapters.save(false);
	chapters.save(true);
	let primary = chapters.slot_path("slot-2.json");
	let previous = chapters.slot_path("slot-2.previous.json");
	let bytes = fs::read(&primary).unwrap();
	let earlier = fs::read(&previous).unwrap();
	let mut edited_garden = chapters.garden.clone();
	edited_garden.entities[0].name.push_str(" changed");
	let inspection = chapters.registry.inspect_slot(
		&chapters.profile.0,
		SaveSlot::Two,
		&chapters.project,
		|_| Ok(edited_garden.clone()),
	);
	assert_eq!(
		inspection.primary,
		ContinueAvailability::IncompatibleRelease
	);
	assert!(inspection.recovery_checkpoint_id.is_none());
	let mut reordered = chapters.project.clone();
	reordered.startup_scene_id = chapters.id.clone();
	let settings = reordered.gameplay_settings.as_mut().unwrap();
	settings["campaign"]["chapters"]
		.as_array_mut()
		.unwrap()
		.swap(0, 1);
	settings["scenes"]["courtyard"]["destination_scene"] = json!("finish");
	settings["scenes"]["garden"]["destination_scene"] = json!("courtyard");
	let registry = CheckpointRegistry::from_project(&reordered)
		.unwrap()
		.unwrap();
	let inspection = chapters.inspect(&registry, &reordered);
	assert_eq!(
		inspection.primary,
		ContinueAvailability::IncompatibleRelease
	);
	assert!(inspection.recovery_checkpoint_id.is_none());
	assert_eq!(fs::read(primary).unwrap(), bytes);
	assert_eq!(fs::read(previous).unwrap(), earlier);
}

#[test]
fn mismatched_prefix_can_recover_v2_but_unknown_record_versions_cannot() {
	let chapters = Chapters::new();
	chapters.save(false);
	chapters.save(true);
	let primary = chapters.slot_path("slot-2.json");
	let previous = chapters.slot_path("slot-2.previous.json");
	let earlier = fs::read(&previous).unwrap();
	let mut fabricated: SaveEnvelope =
		serde_json::from_slice(&fs::read(&primary).unwrap()).unwrap();
	let chapter = fabricated.saved.chapter.take().unwrap();
	let mut value = serde_json::to_value(chapter).unwrap();
	value["completed_chapters"] = json!(["unknown"]);
	fabricated.saved.chapter = Some(serde_json::from_value(value).unwrap());
	fabricated.checksum = save_checksum(&fabricated.saved).unwrap();
	let corrupt = serde_json::to_vec(&fabricated).unwrap();
	fs::write(&primary, &corrupt).unwrap();
	let inspection = chapters.inspect(&chapters.registry, &chapters.project);
	assert_eq!(inspection.primary, ContinueAvailability::Unreadable);
	assert_eq!(inspection.recovery_chapter_label.as_deref(), Some("Garden"));
	let recovered = chapters
		.registry
		.load_slot_for_restore_with_authored(
			&chapters.profile.0,
			SaveSlot::Two,
			&inspection.generation.unwrap(),
			RestoreSource::Previous,
			&chapters.project,
			|_| Ok(chapters.garden.clone()),
		)
		.unwrap();
	assert_eq!(
		chapters
			.registry
			.reconstruct(&recovered, &chapters.project, &chapters.garden)
			.unwrap()
			.1
			.score,
		1
	);
	assert_eq!(fs::read(&primary).unwrap(), corrupt);
	assert_eq!(fs::read(&previous).unwrap(), earlier);
	for unsupported in [SAVE_VERSION, CHAPTER_SAVE_VERSION + 1] {
		fabricated.saved.version = unsupported;
		fs::write(&primary, serde_json::to_vec(&fabricated).unwrap()).unwrap();
		let inspection = chapters.inspect(&chapters.registry, &chapters.project);
		assert_eq!(
			inspection.primary,
			ContinueAvailability::IncompatibleRelease
		);
		assert!(inspection.recovery_checkpoint_id.is_none());
	}
}
