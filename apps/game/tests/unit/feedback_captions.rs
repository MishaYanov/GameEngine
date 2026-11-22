use super::*;

#[test]
fn labels_expire_on_monotonic_deadlines_and_repeats_replace_the_old_entry() {
	let now = Instant::now();
	let mut captions = FeedbackCaptions::default();
	captions.push(CombatCue::Swing, now);
	captions.push(CombatCue::Hit, now + Duration::from_millis(200));
	captions.push(CombatCue::Victory, now + Duration::from_millis(300));
	captions.push(CombatCue::Swing, now + Duration::from_millis(400));
	assert_eq!(captions.entries.len(), 3);
	assert_eq!(
		captions
			.rows(now + Duration::from_millis(400), DisplaySettings::default())
			.collect::<Vec<_>>(),
		["Sword swing", "Victory", "Sword impact"]
	);
	let first_expiry = now + Duration::from_millis(2200);
	assert_eq!(captions.next_expiry(), Some(first_expiry));
	assert!(captions.expire(first_expiry));
	assert!(
		!captions
			.rows(first_expiry, DisplaySettings::default())
			.any(|row| row == "Sword impact")
	);
	assert!(captions.expire(now + Duration::from_millis(2400)));
	assert!(captions.next_expiry().is_none());
}

#[test]
fn new_run_can_clear_all_visible_feedback_without_waiting() {
	let mut captions = FeedbackCaptions::default();
	let now = Instant::now();
	captions.push(CombatCue::Victory, now);
	captions.clear();
	assert_eq!(captions.rows(now, DisplaySettings::default()).next(), None);
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
		&SceneId::new("intro").unwrap(),
	)
		.unwrap();
	assert_eq!(
		audio.cue_details(FeedbackBus::Mechanisms).resolved_percent,
		0
	);
	let now = Instant::now();
	let mut captions = FeedbackCaptions::default();
	captions.push(CombatCue::Hit, now);
	assert_eq!(
		captions
			.rows(now, DisplaySettings::default())
			.collect::<Vec<_>>(),
		["Sword impact"]
	);
}

#[test]
fn disabled_categories_filter_projection_and_remaining_cues_keep_expiry_order() {
	let display: DisplaySettings = serde_json::from_value(serde_json::json!({
		"text_size": "standard",
		"high_contrast": false,
		"cue_captions": true,
		"caption_categories": {"sword": true, "damage": false, "outcome": true}
	}))
		.unwrap();
	let mut captions = FeedbackCaptions::default();
	let now = Instant::now();
	captions.push(CombatCue::Swing, now);
	captions.push(CombatCue::Hurt, now + Duration::from_millis(100));
	captions.push(CombatCue::Defeat, now + Duration::from_millis(200));
	assert_eq!(
		captions
			.rows(now + Duration::from_millis(200), display)
			.collect::<Vec<_>>(),
		["Defeat", "Sword swing"]
	);
	assert_eq!(
		captions.rows(now + LIFETIME, display).collect::<Vec<_>>(),
		["Defeat"]
	);
	assert_eq!(
		captions
			.rows(now + LIFETIME + Duration::from_millis(200), display)
			.next(),
		None
	);
}

#[test]
fn a_hidden_cue_cannot_displace_three_live_visible_captions() {
	let display: DisplaySettings = serde_json::from_value(serde_json::json!({
		"text_size": "standard",
		"high_contrast": false,
		"cue_captions": true,
		"caption_categories": {"sword": true, "damage": true, "outcome": false}
	}))
		.unwrap();
	let mut captions = FeedbackCaptions::default();
	let now = Instant::now();
	for cue in [
		CombatCue::Swing,
		CombatCue::Hit,
		CombatCue::Hurt,
		CombatCue::Victory,
	] {
		captions.push(cue, now);
	}
	assert_eq!(
		captions.rows(now, display).collect::<Vec<_>>(),
		["Hit by the Warden", "Sword impact", "Sword swing"]
	);
	assert_eq!(
		captions
			.rows(now, DisplaySettings::default())
			.collect::<Vec<_>>(),
		["Victory", "Hit by the Warden", "Sword impact"]
	);
}
