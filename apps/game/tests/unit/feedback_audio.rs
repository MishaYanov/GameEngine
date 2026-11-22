use super::*;

fn local_mix(master: u8, [collection, mechanisms, completion]: [u8; 3]) -> FeedbackMix {
	serde_json::from_value(serde_json::json!({
		"level_percent": master,
		"muted": false,
		"bus_percent": { "collection": collection, "mechanisms": mechanisms, "completion": completion }
	}))
		.unwrap()
}

#[test]
fn legacy_clip_is_valid_and_only_gate_has_a_fallback() {
	let player = FeedbackAudio::new(
		None,
		false,
		FeedbackMix::default(),
		&SceneId::new("courtyard").unwrap(),
	)
		.unwrap();
	assert!(player.clip(FeedbackCue::GateOpened).is_some());
	assert!(player.clip(FeedbackCue::KeyCollected).is_none());
	assert!(player.clip(FeedbackCue::RunCompleted).is_none());
}

#[test]
fn applying_mix_rebuilds_from_original_and_zero_clears_cues() {
	let quiet = local_mix(50, [100; 3]);
	let mut audio =
		FeedbackAudio::new(None, false, quiet, &SceneId::new("courtyard").unwrap()).unwrap();
	let original = CueClip::from_wave(GATE_TONE.to_vec()).unwrap();
	assert_eq!(
		audio.clip(FeedbackCue::GateOpened).unwrap().bytes(),
		original.scaled(50).unwrap().bytes()
	);
	assert!(audio.prepare_mix(quiet).unwrap().is_none());

	let muted = audio.prepare_mix(local_mix(0, [100; 3])).unwrap();
	assert!(audio.clip(FeedbackCue::GateOpened).is_some());
	audio.commit_mix(muted);
	assert!(audio.clip(FeedbackCue::GateOpened).is_none());

	let unmuted = audio.prepare_mix(FeedbackMix::default()).unwrap();
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
fn queued_worker() -> (
	Worker,
	std::sync::mpsc::Receiver<(u64, CueClip)>,
	std::sync::mpsc::Receiver<QueuedAudition>,
) {
	let (output, input) = sync_channel(1);
	let (auditions, previews) = sync_channel(1);
	let (wake, _) = sync_channel(1);
	let worker = Worker {
		output,
		auditions,
		epochs: Arc::new(PlaybackEpochs::default()),
		wake,
	};
	(worker, input, previews)
}

#[cfg(windows)]
#[test]
fn a_full_gameplay_queue_never_blocks_and_invalidates_previous_run() {
	let (worker, input, _) = queued_worker();
	let mut player = FeedbackAudio::new(
		None,
		false,
		FeedbackMix::default(),
		&SceneId::new("courtyard").unwrap(),
	)
		.unwrap();
	player.worker = Some(worker);
	let clip = CueClip::from_wave(GATE_TONE.to_vec()).unwrap();
	let worker = player.worker.as_ref().unwrap();
	worker.enqueue(clip.clone());
	worker.enqueue(clip.clone());
	let quiet = player.prepare_mix(local_mix(50, [100; 3])).unwrap();
	player.commit_mix(quiet);
	let worker = player.worker.as_ref().unwrap();
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_ne!(queued_epoch, worker.epochs.gameplay.load(Ordering::Acquire));
	assert!(input.try_recv().is_err());
	worker.enqueue(clip);
	let (queued_epoch, _) = input.try_recv().unwrap();
	assert_eq!(queued_epoch, worker.epochs.gameplay.load(Ordering::Acquire));
}

#[test]
fn auditions_report_missing_silent_and_unavailable_output() {
	let scope = AuditionScope::default();
	let mut player = FeedbackAudio::new(
		None,
		false,
		FeedbackMix::default(),
		&SceneId::new("courtyard").unwrap(),
	)
		.unwrap();
	assert_eq!(
		player.cue_details(FeedbackBus::Mechanisms).clip,
		CueClipSource::BuiltIn
	);
	assert_eq!(
		player.cue_details(FeedbackBus::Collection).clip,
		CueClipSource::Missing
	);
	assert!(matches!(
		player.audition(FeedbackBus::Collection, &scope),
		CueAuditionOutcome::MissingClip
	));
	assert!(matches!(
		player.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::OutputUnavailable
	));
	let silent = player.prepare_mix(local_mix(0, [100; 3])).unwrap();
	player.commit_mix(silent);
	assert_eq!(
		player.cue_details(FeedbackBus::Mechanisms).resolved_percent,
		0
	);
	assert_eq!(
		player.cue_details(FeedbackBus::Mechanisms).clip,
		CueClipSource::BuiltIn
	);
	assert!(matches!(
		player.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::SilentMix
	));
}

#[cfg(windows)]
#[test]
fn menu_audition_uses_prepared_gain_and_drops_its_own_pending_request() {
	use runtime::Key;

	let (worker, input, previews) = queued_worker();
	let mut audio = FeedbackAudio::new(
		None,
		false,
		local_mix(80, [100, 50, 100]),
		&SceneId::new("courtyard").unwrap(),
	)
		.unwrap();
	audio.worker = Some(worker);
	let mut app = crate::GameApp::default();
	app.feedback_audio = audio;
	for key in [
		Key::Escape,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::Enter,
	] {
		assert!(app.player_controls_key(key));
	}
	for _ in 0..12 {
		if app.player_controls_rows(1, 22).unwrap() == ["> Test Mechanisms"] {
			break;
		}
		app.player_controls_key(Key::ArrowDown);
	}
	assert_eq!(
		app.player_controls_rows(1, 22).unwrap(),
		["> Test Mechanisms"]
	);
	assert!(app.player_controls_key(Key::Enter));
	assert_eq!(
		app.player_controls_rows(2, 22).unwrap()[1],
		"Cue test submitted"
	);
	assert!(app.player_controls_key(Key::Enter));
	assert_eq!(
		app.player_controls_rows(2, 24).unwrap()[1],
		"Queue unavailable"
	);
	let queued = previews.try_recv().unwrap();
	let epochs = Arc::clone(&app.feedback_audio.worker.as_ref().unwrap().epochs);
	assert!(queued.is_current(&epochs));
	assert_eq!(
		queued.clip.bytes(),
		app.feedback_audio
			.clip(FeedbackCue::GateOpened)
			.unwrap()
			.bytes()
	);
	assert!(input.try_recv().is_err());
	assert!(previews.try_recv().is_err());
	assert!(
		app.feedback_captions
			.rows(std::time::Instant::now())
			.next()
			.is_none()
	);

	// Gameplay retains its independent queue and generation when leaving the menu.
	app.feedback_audio.emit(FeedbackCue::GateOpened);
	assert!(app.player_controls_key(Key::Escape));
	assert!(!queued.is_current(&epochs));
	let (gameplay_epoch, _) = input.try_recv().unwrap();
	assert_eq!(gameplay_epoch, epochs.gameplay.load(Ordering::Acquire));
}

#[cfg(windows)]
#[test]
fn admitted_scene_and_factor_changes_cancel_auditions_even_when_gains_match() {
	let (worker, _, previews) = queued_worker();
	let current = SceneId::new("courtyard").unwrap();
	let garden = SceneId::new("garden").unwrap();
	let mut audio = FeedbackAudio::new(None, false, local_mix(50, [50; 3]), &current).unwrap();
	audio.worker = Some(worker);
	let epochs = Arc::clone(&audio.worker.as_ref().unwrap().epochs);
	let scope = AuditionScope::default();
	assert!(matches!(
		audio.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::Submitted
	));
	let queued = previews.try_recv().unwrap();
	assert!(queued.is_current(&epochs));
	let prepared = audio
		.prepare_scene_mix(local_mix(50, [50; 3]), &garden)
		.unwrap();
	assert!(prepared.is_none());
	assert!(queued.is_current(&epochs));
	audio.commit_scene_mix(garden, prepared);
	assert!(!queued.is_current(&epochs));

	assert!(matches!(
		audio.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::Submitted
	));
	let queued = previews.try_recv().unwrap();
	assert!(queued.is_current(&epochs));
	let gameplay_epoch = epochs.gameplay.load(Ordering::Acquire);
	let prepared = audio.prepare_mix(local_mix(25, [100; 3])).unwrap();
	assert_eq!(prepared.as_ref().unwrap().gains, audio.mix.gains);
	audio.commit_mix(prepared);
	assert!(!queued.is_current(&epochs));
	assert_eq!(epochs.gameplay.load(Ordering::Acquire), gameplay_epoch);
	let factors = audio.cue_details(FeedbackBus::Mechanisms).factors;
	assert_eq!((factors.master_percent, factors.local_percent), (25, 100));
}

#[test]
fn a_scene_mix_commits_only_after_admission_and_retains_the_scene_for_local_changes() {
	let root =
		std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/courtyard-audio/assets");
	let source = bridge::feedback_audio::load(&root).unwrap().unwrap();
	let settings = serde_json::from_str(
		r#"{
		"bus_percent": {"mechanisms": 50},
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
	let local = local_mix(80, [100, 75, 60]);
	let mut player = FeedbackAudio::new(Some(catalog), false, local, &courtyard).unwrap();
	let cue = FeedbackCue::GateOpened;
	let original = source.clip(cue).unwrap();
	assert_eq!(player.mix.gains, [80, 30, 48]);
	assert_eq!(
		player.cue_details(FeedbackBus::Mechanisms),
		CueGainDetails {
			factors: CueGainFactors {
				master_percent: 80,
				local_percent: 75,
				project_percent: 50,
				scene_percent: 100,
			},
			resolved_percent: 30,
			clip: CueClipSource::Authored,
		}
	);
	assert_eq!(
		player.mix.clips.iter().flatten().count(),
		FeedbackCue::ALL.len()
	);
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(30).unwrap().bytes()
	);

	assert!(
		player
			.prepare_scene_mix(local_mix(80, [101, 75, 60]), &garden)
			.is_err()
	);
	let pending = player.prepare_scene_mix(local, &garden).unwrap();
	assert_eq!(player.mix.gains, [80, 30, 48]);
	assert_eq!(player.scene.as_ref(), Some(&courtyard));
	player.commit_scene_mix(garden.clone(), pending);
	assert_eq!(player.scene.as_ref(), Some(&garden));
	let details = player.cue_details(FeedbackBus::Mechanisms);
	assert_eq!(details.factors.scene_percent, 50);
	assert_eq!(details.resolved_percent, 15);
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(15).unwrap().bytes()
	);
	assert_eq!(player.mix.clips.iter().flatten().count(), 3);

	let reusable = player.prepare_scene_mix(local, &other).unwrap();
	assert!(reusable.is_none());
	player.commit_scene_mix(other.clone(), reusable);
	assert_eq!(player.scene.as_ref(), Some(&other));
	let cached_completion = player
		.clip(FeedbackCue::RunCompleted)
		.unwrap()
		.bytes()
		.as_ptr();
	let one_silent_category = player.prepare_mix(local_mix(80, [0, 75, 60])).unwrap();
	player.commit_mix(one_silent_category);
	assert_eq!(player.mix.gains, [0, 15, 48]);
	assert!(player.clip(FeedbackCue::KeyCollected).is_none());
	assert_eq!(
		player
			.clip(FeedbackCue::RunCompleted)
			.unwrap()
			.bytes()
			.as_ptr(),
		cached_completion,
	);

	let mut muted = serde_json::to_value(local_mix(80, [0, 75, 60])).unwrap();
	muted["muted"] = true.into();
	let mute = player
		.prepare_mix(serde_json::from_value(muted).unwrap())
		.unwrap();
	player.commit_mix(mute);
	assert!(player.mix.clips.iter().all(Option::is_none));
	let local = player.prepare_mix(FeedbackMix::default()).unwrap();
	player.commit_mix(local);
	assert_eq!(player.mix.gains, [100, 25, 100]);
	assert_eq!(
		player.clip(cue).unwrap().bytes(),
		original.scaled(25).unwrap().bytes()
	);
}

#[test]
fn local_bus_and_authored_gain_round_once() {
	assert_eq!(resolved_bus_percent(80, 75, 50, 100), 30);
	assert_eq!(resolved_bus_percent(1, 50, 99, 100), 0);
	assert_eq!(resolved_bus_percent(1, 50, 100, 100), 1);
	assert_eq!(resolved_bus_percent(100, 100, 100, 100), 100);
}
