use super::*;

fn write_save(
	registry: &CheckpointRegistry,
	profile: &Path,
	project: &ProjectData,
	authored: &SceneData,
	score: i32,
) {
	let progress = CampaignProgress {
		score,
		has_courtyard_key: true,
		..CampaignProgress::default()
	};
	let checkpoint = registry
		.active(&project.startup_scene_id, authored, &progress)
		.unwrap();
	registry
		.save(profile, checkpoint, project, authored, &progress)
		.unwrap();
}

#[test]
fn transfer_uses_exact_bytes_and_fails_closed_when_review_changes() {
	let (project, authored, registry) = fixture();
	let source = ProfileDir::new();
	let target = ProfileDir::new();
	let export = source.0.join("portable.json");
	let slot = SaveSlot::One;
	let load = |_: &SceneId| Ok(authored.clone());
	write_save(&registry, &source.0, &project, &authored, 1);
	write_save(&registry, &target.0, &project, &authored, 2);
	let old = registry.read_primary(&target.0, slot).unwrap().unwrap();
	let original = registry.read_primary(&source.0, slot).unwrap().unwrap();
	let generation = registry
		.inspect_slot(&source.0, slot, &project, load)
		.generation
		.unwrap();
	let bytes = registry
		.export_slot_transfer(&source.0, slot, &generation, &project, load)
		.unwrap();
	let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
	assert_eq!(json["record_utf8"].as_str().unwrap().as_bytes(), original);
	fs::write(&export, &bytes).unwrap();
	let preview = registry
		.preview_slot_transfer(&target.0, slot, &export, &project, load)
		.unwrap();
	assert!(preview.replacement_required);
	let review = ImportReview::from_hex(
		&preview.archive_sha256,
		&preview.destination_generation_sha256,
	)
		.unwrap();
	let parent = registry.slot_parent(&target.0, false).unwrap().unwrap();
	let reject = |intent| {
		registry.import_slot_transfer(
			CheckpointImport {
				profile: &target.0,
				slot,
				archive: &export,
				review: &review,
				intent,
				project: &project,
			},
			load,
		)
	};
	assert!(
		reject(ImportIntent::EmptyOnly)
			.unwrap_err()
			.contains("Explicit replacement")
	);
	let mut modified = bytes.clone();
	modified.push(b'\n');
	fs::write(&export, &modified).unwrap();
	assert!(
		reject(ImportIntent::ReplaceCurrent)
			.unwrap_err()
			.contains("transfer changed")
	);
	fs::write(&export, &bytes).unwrap();
	fs::write(parent.join(slot.previous()), &old).unwrap();
	assert!(
		reject(ImportIntent::ReplaceCurrent)
			.unwrap_err()
			.contains("slot changed")
	);
	fs::remove_file(parent.join(slot.previous())).unwrap();

	let interrupted = registry.publish_transfer(
		CheckpointImport {
			profile: &target.0,
			slot,
			archive: &export,
			review: &review,
			intent: ImportIntent::ReplaceCurrent,
			project: &project,
		},
		load,
		|parent, filename, data| {
			if filename == slot.primary() {
				return Err("simulated save publication failure".into());
			}
			publish_record(parent, filename, data)
		},
	);
	assert!(interrupted.is_err());
	assert_eq!(fs::read(parent.join(slot.primary())).unwrap(), old);
	assert_eq!(fs::read(parent.join(slot.previous())).unwrap(), old);

	let refreshed = registry
		.preview_slot_transfer(&target.0, slot, &export, &project, load)
		.unwrap();
	let review = ImportReview::from_hex(
		&refreshed.archive_sha256,
		&refreshed.destination_generation_sha256,
	)
		.unwrap();
	let replacement = || CheckpointImport {
		profile: &target.0,
		slot,
		archive: &export,
		review: &review,
		intent: ImportIntent::ReplaceCurrent,
		project: &project,
	};
	let held = lock_slot(&parent, slot).unwrap();
	assert!(
		registry
			.import_slot_transfer(replacement(), load)
			.unwrap_err()
			.contains("lock")
	);
	drop(held);
	registry.import_slot_transfer(replacement(), load).unwrap();
	assert_eq!(fs::read(parent.join(slot.primary())).unwrap(), original);
	assert_eq!(fs::read(parent.join(slot.previous())).unwrap(), old);
	assert_eq!(
		registry.read_primary(&source.0, slot).unwrap().unwrap(),
		original
	);
	assert_eq!(fs::read(export).unwrap(), bytes);
}

#[test]
fn over_limit_and_incompatible_authored_content_leave_empty_destination_alone() {
	let (project, authored, registry) = fixture();
	let profile = ProfileDir::new();
	let file = profile.0.join("portable.json");
	fs::write(&file, vec![b' '; 32769]).unwrap();
	assert!(
		registry
			.preview_slot_transfer(&profile.0, SaveSlot::Two, &file, &project, |_| Ok(
				authored.clone()
			))
			.unwrap_err()
			.contains("size limit")
	);
	write_save(&registry, &profile.0, &project, &authored, 1);
	let expected = registry
		.inspect_slot(
			&profile.0,
			SaveSlot::One,
			&project,
			|_| Ok(authored.clone()),
		)
		.generation
		.unwrap();
	let original = registry
		.export_slot_transfer(&profile.0, SaveSlot::One, &expected, &project, |_| {
			Ok(authored.clone())
		})
		.unwrap();
	fs::write(&file, &original).unwrap();
	let mut changed = authored.clone();
	changed.entities[0].name.push_str(" changed");
	assert!(
		registry
			.preview_slot_transfer(&profile.0, SaveSlot::Two, &file, &project, |_| Ok(
				changed.clone()
			))
			.is_err()
	);
	let mut wrapper: serde_json::Value = serde_json::from_slice(&original).unwrap();
	wrapper["record_sha256"] = json!("1".repeat(64));
	fs::write(&file, serde_json::to_vec(&wrapper).unwrap()).unwrap();
	assert!(
		registry
			.preview_slot_transfer(&profile.0, SaveSlot::Two, &file, &project, |_| Ok(
				authored.clone()
			))
			.unwrap_err()
			.contains("digest")
	);
	assert!(
		registry
			.read_primary(&profile.0, SaveSlot::Two)
			.unwrap()
			.is_none()
	);
}

#[test]
fn reviewed_empty_slot_is_bound_to_the_canonical_destination_profile() {
	let (project, authored, registry) = fixture();
	let source = ProfileDir::new();
	let destination = ProfileDir::new();
	let unrelated = ProfileDir::new();
	write_save(&registry, &source.0, &project, &authored, 1);
	let load = |_: &SceneId| Ok(authored.clone());
	let source_slot = SaveSlot::One;
	let target_slot = SaveSlot::Two;
	let generation = registry
		.slot_transfer_generation(&source.0, source_slot)
		.unwrap();
	let transfer = source.0.join("checkpoint.json");
	fs::write(
		&transfer,
		registry
			.export_slot_transfer(&source.0, source_slot, &generation, &project, load)
			.unwrap(),
	)
		.unwrap();
	let preview = registry
		.preview_slot_transfer(&destination.0, target_slot, &transfer, &project, load)
		.unwrap();
	let reviewed = ImportReview::from_hex(
		&preview.archive_sha256,
		&preview.destination_generation_sha256,
	)
		.unwrap();
	assert!(
		registry
			.import_slot_transfer(
				CheckpointImport {
					profile: &unrelated.0,
					slot: target_slot,
					archive: &transfer,
					review: &reviewed,
					intent: ImportIntent::EmptyOnly,
					project: &project,
				},
				load,
			)
			.is_err()
	);
	assert!(fs::read_dir(&unrelated.0).unwrap().next().is_none());
	registry
		.import_slot_transfer(
			CheckpointImport {
				profile: &destination.0,
				slot: target_slot,
				archive: &transfer,
				review: &reviewed,
				intent: ImportIntent::EmptyOnly,
				project: &project,
			},
			load,
		)
		.unwrap();
}
