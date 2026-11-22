use super::*;

#[test]
fn legacy_clip_is_valid_and_only_gate_has_a_fallback() {
	let player = FeedbackAudio::new(None, false, 100, &SceneId::new("courtyard").unwrap()).unwrap();
	assert!(player.clip(FeedbackCue::GateOpened).is_some());
	assert!(player.clip(FeedbackCue::KeyCollected).is_none());
	assert!(player.clip(FeedbackCue::RunCompleted).is_none());
}

#[test]
fn applying_mix_rebuilds_from_original_and_zero_clears_cues() {
	let mut audio =
		FeedbackAudio::new(None, false, 50, &SceneId::new("courtyard").unwrap()).unwrap();
	let original = CueClip::from_wave(GATE_TONE.to_vec()).unwrap();
	assert_eq!(
		audio.clip(FeedbackCue::GateOpened).unwrap().bytes(),
		original.scaled(50).unwrap().bytes()
	);
	assert!(audio.prepare_mix(50).unwrap().is_none());

	let muted = audio.prepare_mix(0).unwrap();
	assert!(audio.clip(FeedbackCue::GateOpened).is_some());
	audio.commit_mix(muted);
	assert!(audio.clip(FeedbackCue::GateOpened).is_none());

	let unmuted = audio.prepare_mix(100).unwrap();
	audio.commit_mix(unmuted);
	assert_eq!(
		audio.clip(FeedbackCue::GateOpened).unwrap().bytes(),
		GATE_TONE
	);
}

#[test]
fn committed_facts_emit_each_edge_once_in_order() {
	let before = Facts {
		key: false,
		gate: false,
		complete: false,
	};
	let after = Facts {
		key: true,
		gate: true,
		complete: true,
	};
	assert_eq!(
		before.rising(after).collect::<Vec<_>>(),
		[
			FeedbackCue::KeyCollected,
			FeedbackCue::GateOpened,
			FeedbackCue::RunCompleted
		]
	);
	assert_eq!(after.rising(after).count(), 0);
	assert_eq!(after.rising(before).count(), 0);
}

#[cfg(windows)]
#[test]
fn a_full_queue_never_blocks_and_invalidates_previous_run() {
	let (output, input) = sync_channel(1);
	let worker = Worker {
		output,
		epoch: Arc::new(AtomicU64::new(0)),
	};
	let mut player = FeedbackAudio::default();
	player.worker = Some(worker);
	let clip = CueClip::from_wave(GATE_TONE.to_vec()).unwrap();
	let worker = player.worker.as_ref().unwrap();
	worker.enqueue(clip.clone());
	worker.enqueue(clip.clone());
	let muted = player.prepare_mix(50).unwrap();
	player.commit_mix(muted);
	let worker = player.worker.as_ref().unwrap();
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_ne!(queued_epoch, worker.epoch.load(Ordering::Relaxed));
	assert!(input.try_recv().is_err());
	worker.enqueue(clip);
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_eq!(queued_epoch, worker.epoch.load(Ordering::Relaxed));
}

#[test]
fn a_scene_mix_commits_only_after_admission_and_retains_the_scene_for_local_changes() {
	let root =
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/courtyard-audio/assets");
	let source = bridge::feedback_audio::load(&root).unwrap().unwrap();
	let settings = serde_json::from_str(
		r#"{
		"bus_percent": {"mechanisms": 80},
		"scene_bus_percent": {"garden": {"mechanisms": 50}, "other": {"mechanisms": 50}}
	}"#,
	)
	.unwrap();
	let catalog = bridge::feedback_audio::prepare_catalog_with_mix(
		&root,
		source.assignments().collect(),
		settings,
	)
	.unwrap();
	let courtyard = SceneId::new("courtyard").unwrap();
	let garden = SceneId::new("garden").unwrap();
	let other = SceneId::new("other").unwrap();
	let mut player = FeedbackAudio::new(Some(catalog), false, 75, &courtyard).unwrap();
	let cue = FeedbackCue::GateOpened;
	let original = source.clip(cue).unwrap();
	assert_eq!(player.mix.gains, [75, 60, 75]);
	assert_eq!(
		player.mix.clips.iter().flatten().count(),
		FeedbackCue::ALL.len()
	);
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(60).unwrap().bytes()
	);

	assert!(player.prepare_scene_mix(101, &garden).is_err());
	let pending = player.prepare_scene_mix(75, &garden).unwrap();
	assert_eq!(player.mix.gains, [75, 60, 75]);
	assert_eq!(player.scene.as_ref(), Some(&courtyard));
	player.commit_scene_mix(garden.clone(), pending);
	assert_eq!(player.scene.as_ref(), Some(&garden));
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(30).unwrap().bytes()
	);
	assert_eq!(player.mix.clips.iter().flatten().count(), 3);

	let reusable = player.prepare_scene_mix(75, &other).unwrap();
	assert!(reusable.is_none());
	player.commit_scene_mix(other.clone(), reusable);
	assert_eq!(player.scene.as_ref(), Some(&other));
	let mute = player.prepare_mix(0).unwrap();
	player.commit_mix(mute);
	assert!(player.mix.clips.iter().all(Option::is_none));
	let local = player.prepare_mix(100).unwrap();
	player.commit_mix(local);
	assert_eq!(player.mix.gains, [100, 40, 100]);
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(40).unwrap().bytes()
	);
}
