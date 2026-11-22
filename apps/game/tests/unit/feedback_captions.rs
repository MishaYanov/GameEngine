use super::*;

#[test]
fn labels_expire_on_monotonic_deadlines_and_repeats_replace_the_old_entry() {
	let now = Instant::now();
	let mut captions = FeedbackCaptions::default();
	captions.push(FeedbackCue::KeyCollected, now);
	captions.push(FeedbackCue::GateOpened, now + Duration::from_millis(200));
	captions.push(FeedbackCue::RunCompleted, now + Duration::from_millis(300));
	captions.push(FeedbackCue::KeyCollected, now + Duration::from_millis(400));
	assert_eq!(captions.entries.len(), 3);
	assert_eq!(
		captions
			.rows(now + Duration::from_millis(400))
			.collect::<Vec<_>>(),
		["Key collected", "Run completed", "Gate opened"]
	);
	let first_expiry = now + Duration::from_millis(2200);
	assert_eq!(captions.next_expiry(), Some(first_expiry));
	assert!(captions.expire(first_expiry));
	assert!(!captions.rows(first_expiry).any(|row| row == "Gate opened"));
	assert!(captions.expire(now + Duration::from_millis(2400)));
	assert!(captions.next_expiry().is_none());
}

#[test]
fn new_run_can_clear_all_visible_feedback_without_waiting() {
	let mut captions = FeedbackCaptions::default();
	let now = Instant::now();
	captions.push(FeedbackCue::RunCompleted, now);
	captions.clear();
	assert_eq!(captions.rows(now).next(), None);
	assert_eq!(captions.next_expiry(), None);
}

#[test]
fn zero_audio_gain_still_allows_gameplay_captions() {
	use bridge::feedback_audio::FeedbackBus;
	use runtime::scene_catalog::SceneId;

	let mix = serde_json::from_value(serde_json::json!({
		"level_percent": 0,
		"muted": false,
		"bus_percent": {"collection": 100, "mechanisms": 100, "completion": 100}
	}))
		.unwrap();
	let audio = crate::feedback_audio::FeedbackAudio::new(
		None,
		false,
		mix,
		&SceneId::new("courtyard").unwrap(),
	)
		.unwrap();
	assert_eq!(
		audio.cue_details(FeedbackBus::Mechanisms).resolved_percent,
		0
	);
	let now = Instant::now();
	let mut captions = FeedbackCaptions::default();
	let before = crate::feedback_audio::Facts {
		key: false,
		gate: false,
		complete: false,
	};
	let after = crate::feedback_audio::Facts {
		gate: true,
		..before
	};
	for cue in before.rising(after) {
		captions.push(cue, now);
	}
	assert_eq!(captions.rows(now).collect::<Vec<_>>(), ["Gate opened"]);
}
