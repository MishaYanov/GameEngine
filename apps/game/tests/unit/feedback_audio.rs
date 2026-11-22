use super::*;
use bridge::feedback_audio::{CueAssignment, FeedbackBusMix, prepare_catalog_with_mix};
use std::{fs, path::PathBuf};

const CUE_PATH: &str = "audio/cue-test.wav";
const TEST_CLIP: &[u8] = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\x3e\0\0\0\x7d\0\0\x02\0\x10\0data\x02\0\0\0\x5a\x13";

struct CueFixture(PathBuf);

impl CueFixture {
	fn new() -> Self {
		let folder = std::env::temp_dir().join(format!(
			"player-cue-audition-{}",
			runtime::EntityKey::generate().as_str()
		));
		fs::create_dir(&folder).unwrap();
		fs::create_dir(folder.join("audio")).unwrap();
		fs::write(folder.join(CUE_PATH), TEST_CLIP).unwrap();
		Self(folder)
	}

	fn catalog(&self, cues: &[FeedbackCue], mix: FeedbackBusMix) -> PreparedCueCatalog {
		let assignments = cues
			.iter()
			.map(|&event| CueAssignment {
				event,
				path: CUE_PATH.into(),
			})
			.collect();
		prepare_catalog_with_mix(&self.0, assignments, mix).unwrap()
	}
}

impl Drop for CueFixture {
	fn drop(&mut self) {
		let _ = fs::remove_file(self.0.join(CUE_PATH));
		let _ = fs::remove_dir(self.0.join("audio"));
		let _ = fs::remove_dir(&self.0);
	}
}

fn local_mix(master: u8, [collection, mechanisms, completion]: [u8; 3]) -> FeedbackMix {
	serde_json::from_value(serde_json::json!({
		"level_percent": master,
		"muted": false,
		"bus_percent": { "collection": collection, "mechanisms": mechanisms, "completion": completion }
	}))
		.unwrap()
}

#[test]
fn a_project_without_an_authored_catalog_has_no_fallback() {
	let player = FeedbackAudio::new(
		None,
		true,
		FeedbackMix::default(),
		&SceneId::new("intro").unwrap(),
	)
		.unwrap();
	for cue in FeedbackCue::ALL {
		assert!(player.clip(cue).is_none());
		assert_eq!(player.cue_details(cue.bus()).clip, CueClipSource::Missing);
	}
	#[cfg(windows)]
	assert!(player.worker.is_none());
}

#[test]
fn applying_mix_rebuilds_from_original_and_zero_clears_cues() {
	let quiet = local_mix(50, [100; 3]);
	let catalog = CueFixture::new().catalog(&[FeedbackCue::GateOpened], FeedbackBusMix::default());
	let mut audio =
		FeedbackAudio::new(Some(catalog), false, quiet, &SceneId::new("intro").unwrap()).unwrap();
	let original = CueClip::from_wave(TEST_CLIP.to_vec()).unwrap();
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
		TEST_CLIP
	);
}

#[cfg(windows)]
fn queued_worker() -> (Worker, std::sync::mpsc::Receiver<QueuedAudition>) {
	let (auditions, previews) = sync_channel(1);
	let worker = Worker {
		auditions,
		epoch: Arc::new(AtomicU64::new(0)),
	};
	(worker, previews)
}

#[test]
fn auditions_report_missing_silent_and_unavailable_output() {
	let scope = AuditionScope::default();
	let catalog = CueFixture::new().catalog(&[FeedbackCue::GateOpened], FeedbackBusMix::default());
	let mut player = FeedbackAudio::new(
		Some(catalog),
		false,
		FeedbackMix::default(),
		&SceneId::new("intro").unwrap(),
	)
		.unwrap();
	assert_eq!(
		player.cue_details(FeedbackBus::Mechanisms).clip,
		CueClipSource::Authored
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
		CueClipSource::Authored
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

	let (worker, previews) = queued_worker();
	let catalog = CueFixture::new().catalog(&[FeedbackCue::GateOpened], FeedbackBusMix::default());
	let mut audio = FeedbackAudio::new(
		Some(catalog),
		false,
		local_mix(80, [100, 50, 100]),
		&SceneId::new("intro").unwrap(),
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
	let epoch = Arc::clone(&app.feedback_audio.worker.as_ref().unwrap().epoch);
	assert!(queued.is_current(&epoch));
	assert_eq!(
		queued.clip.bytes(),
		app.feedback_audio
			.clip(FeedbackCue::GateOpened)
			.unwrap()
			.bytes()
	);
	assert!(previews.try_recv().is_err());
	assert!(app.player_controls_key(Key::Escape));
	assert!(!queued.is_current(&epoch));
}

#[cfg(windows)]
#[test]
fn admitted_scene_and_factor_changes_cancel_auditions_even_when_gains_match() {
	let (worker, previews) = queued_worker();
	let current = SceneId::new("intro").unwrap();
	let arena = SceneId::new("arena").unwrap();
	let catalog = CueFixture::new().catalog(&[FeedbackCue::GateOpened], FeedbackBusMix::default());
	let mut audio =
		FeedbackAudio::new(Some(catalog), false, local_mix(50, [50; 3]), &current).unwrap();
	audio.worker = Some(worker);
	let epoch = Arc::clone(&audio.worker.as_ref().unwrap().epoch);
	let scope = AuditionScope::default();
	assert!(matches!(
		audio.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::Submitted
	));
	let queued = previews.try_recv().unwrap();
	assert!(queued.is_current(&epoch));
	let prepared = audio
		.prepare_scene_mix(local_mix(50, [50; 3]), &arena)
		.unwrap();
	assert!(prepared.is_none());
	assert!(queued.is_current(&epoch));
	audio.commit_scene_mix(arena, prepared);
	assert!(!queued.is_current(&epoch));

	assert!(matches!(
		audio.audition(FeedbackBus::Mechanisms, &scope),
		CueAuditionOutcome::Submitted
	));
	let queued = previews.try_recv().unwrap();
	assert!(queued.is_current(&epoch));
	let prepared = audio.prepare_mix(local_mix(25, [100; 3])).unwrap();
	assert_eq!(prepared.as_ref().unwrap().gains, audio.mix.gains);
	audio.commit_mix(prepared);
	assert!(!queued.is_current(&epoch));
	let factors = audio.cue_details(FeedbackBus::Mechanisms).factors;
	assert_eq!((factors.master_percent, factors.local_percent), (25, 100));
}

#[test]
fn a_scene_mix_commits_only_after_admission_and_retains_the_scene_for_local_changes() {
	let settings = serde_json::from_str(
		r#"{
		"bus_percent": {"mechanisms": 50},
		"scene_bus_percent": {"arena": {"mechanisms": 50}, "other": {"mechanisms": 50}}
	}"#,
	)
	.unwrap();
	let catalog = CueFixture::new().catalog(&FeedbackCue::ALL, settings);
	let intro = SceneId::new("intro").unwrap();
	let arena = SceneId::new("arena").unwrap();
	let other = SceneId::new("other").unwrap();
	let local = local_mix(80, [100, 75, 60]);
	let mut player = FeedbackAudio::new(Some(catalog), false, local, &intro).unwrap();
	let cue = FeedbackCue::GateOpened;
	let original = CueClip::from_wave(TEST_CLIP.to_vec()).unwrap();
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
			.prepare_scene_mix(local_mix(80, [101, 75, 60]), &arena)
			.is_err()
	);
	let pending = player.prepare_scene_mix(local, &arena).unwrap();
	assert_eq!(player.mix.gains, [80, 30, 48]);
	assert_eq!(player.scene.as_ref(), Some(&intro));
	player.commit_scene_mix(arena.clone(), pending);
	assert_eq!(player.scene.as_ref(), Some(&arena));
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
