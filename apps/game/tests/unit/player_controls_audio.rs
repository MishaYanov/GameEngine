use super::*;
use crate::GameApp;

fn focus_row(page: &mut AudioMenu, label: &str) {
	for _ in 0..AudioMenu::ROWS.len() {
		let (_, rows, focus) = page.settings_rows();
		if rows[focus].starts_with(label) {
			return;
		}
		page.key(Key::ArrowDown, &ProjectSound::default());
	}
	panic!("Missing audio row {label}");
}

#[test]
fn player_audio_menu_clamps_volume_and_retains_it_beneath_mute() {
	let mut page = AudioMenu::new(FeedbackMix::default(), false);
	assert!(matches!(
		page.key(Key::ArrowRight, &ProjectSound::default()),
		AudioAction::Stay
	));
	assert_eq!(page.settings_rows().1[0], "Volume: 100%");
	for _ in 0..12 {
		page.key(Key::ArrowLeft, &ProjectSound::default());
	}
	assert_eq!(page.settings_rows().1[0], "Volume: 0%");
	for _ in 0..3 {
		page.key(Key::ArrowRight, &ProjectSound::default());
	}
	page.key(Key::ArrowDown, &ProjectSound::default());
	page.key(Key::Enter, &ProjectSound::default());
	assert_eq!(page.settings_rows().1[1], "Mute: On");
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(muted) = page.key(Key::Enter, &ProjectSound::default()) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(muted.level_percent, 30);
	assert_eq!(muted.effective_percent(), 0);
	focus_row(&mut page, "Mute");
	page.key(Key::ArrowLeft, &ProjectSound::default());
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(audible) = page.key(Key::Enter, &ProjectSound::default()) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(audible.effective_percent(), 30);
}

#[test]
fn restoring_category_defaults_edits_only_the_draft_categories() {
	let mut page = AudioMenu::new(FeedbackMix::default(), false);
	page.key(Key::ArrowLeft, &ProjectSound::default());
	focus_row(&mut page, "Mute");
	page.key(Key::Enter, &ProjectSound::default());

	for bus in [
		FeedbackBus::Collection,
		FeedbackBus::Mechanisms,
		FeedbackBus::Completion,
	] {
		focus_row(&mut page, bus_label(bus));
		for _ in 0..12 {
			page.key(Key::ArrowLeft, &ProjectSound::default());
		}
		assert_eq!(page.draft.category_percent(bus), 0);
	}

	focus_row(&mut page, "Restore Category Defaults");
	assert!(matches!(
		page.key(Key::Enter, &ProjectSound::default()),
		AudioAction::Stay
	));
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(restored) = page.key(Key::Enter, &ProjectSound::default()) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(restored.level_percent, 90);
	assert!(restored.muted);
	assert_eq!(restored.bus_percent, LocalBusLevels::default());
}

#[test]
fn back_discards_feedback_audio_and_failed_save_preserves_applied_mix() {
	let mut app = GameApp::default();
	for key in [
		Key::Escape,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::Enter,
	] {
		assert!(app.player_controls_key(key));
	}
	app.player_controls_key(Key::ArrowLeft);
	assert_eq!(
		app.player_controls.rows_with_layout(2, 22).unwrap()[0],
		"> Volume: 90%"
	);
	app.player_controls_key(Key::Escape);
	assert_eq!(
		app.player_controls.rows_with_layout(1, 22).unwrap(),
		["> Feedback Audio"]
	);
	app.player_controls_key(Key::Enter);
	assert_eq!(
		app.player_controls.rows_with_layout(2, 22).unwrap()[0],
		"> Volume: 100%"
	);
	for key in [Key::ArrowDown, Key::ArrowDown, Key::ArrowLeft] {
		app.player_controls_key(key);
	}
	assert_eq!(
		app.player_controls.rows_with_layout(1, 22).unwrap(),
		["> Collection: 90%"]
	);
	for _ in 0..4 {
		app.player_controls_key(Key::ArrowDown);
	}
	app.player_controls_key(Key::Enter);
	assert_eq!(app.player_controls.feedback_mix(), FeedbackMix::default());
	assert!(app.player_controls.rows_with_layout(2, 40).unwrap()[1].contains("storage"));
}

#[cfg(windows)]
#[test]
fn dpad_can_adjust_volume_and_confirm_mute_without_using_keyboard() {
	use runtime::input::{MenuButtons, PhysicalGamepadSample};

	let mut app = GameApp::default();
	let press = |app: &mut GameApp, menu, face_buttons| {
		app.player_controls_gamepad(PhysicalGamepadSample::default());
		assert!(app.player_controls_gamepad(PhysicalGamepadSample {
			menu,
			face_buttons,
			..PhysicalGamepadSample::default()
		}));
	};
	press(
		&mut app,
		MenuButtons {
			open: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	for _ in 0..3 {
		press(
			&mut app,
			MenuButtons {
				up: true,
				..MenuButtons::default()
			},
			[false; 4],
		);
	}
	press(
		&mut app,
		MenuButtons::default(),
		[true, false, false, false],
	);
	press(
		&mut app,
		MenuButtons {
			left: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	assert_eq!(
		app.player_controls.rows_with_layout(2, 22).unwrap(),
		["> Volume: 90%", "A: select  B: back"]
	);
	press(
		&mut app,
		MenuButtons {
			down: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	press(
		&mut app,
		MenuButtons::default(),
		[true, false, false, false],
	);
	assert_eq!(
		app.player_controls.rows_with_layout(1, 22).unwrap(),
		["> Mute: On"]
	);
	press(
		&mut app,
		MenuButtons {
			down: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	press(
		&mut app,
		MenuButtons {
			left: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	assert_eq!(
		app.player_controls.rows_with_layout(1, 22).unwrap(),
		["> Collection: 90%"]
	);
	press(
		&mut app,
		MenuButtons::default(),
		[false, true, false, false],
	);
	assert_eq!(app.player_controls.feedback_mix(), FeedbackMix::default());
}

#[test]
fn cue_details_show_current_audio_and_caption_settings_independently() {
	use runtime::scene_catalog::SceneId;

	let committed = FeedbackMix {
		muted: true,
		..FeedbackMix::default()
	};
	let audio =
		FeedbackAudio::new(None, false, committed, &SceneId::new("garden").unwrap()).unwrap();
	let mut page = AudioMenu::new(FeedbackMix::default(), false);
	focus_row(&mut page, "Cue Details");
	page.key(Key::Enter, &ProjectSound::default());
	let (_, rows, focus) = page.rows(&audio, &ProjectSound::default(), committed, false);
	assert_eq!(rows[focus], "Category: Collection");
	assert!(rows.contains(&"Clip: Missing".into()));
	assert!(rows.contains(&"Cue captions: Off".into()));

	page.key(Key::ArrowRight, &ProjectSound::default());
	let (_, rows, focus) = page.rows(&audio, &ProjectSound::default(), committed, true);
	assert_eq!(rows[focus], "Category: Mechanisms");
	assert!(rows.contains(&"Master: 0%".into()));
	assert!(rows.contains(&"Resolved: 0%".into()));
	assert!(rows.contains(&"Clip: Missing".into()));
	assert!(rows.contains(&"Cue captions: On".into()));

	page.key(Key::Escape, &ProjectSound::default());
	let (title, rows, focus) = page.rows(&audio, &ProjectSound::default(), committed, true);
	assert_eq!(title, "Feedback Audio");
	assert_eq!(rows[focus], "Cue Details");
}

#[test]
fn cinder_audio_check_uses_authored_cues_and_applied_mix_without_touching_gameplay() {
	use crate::project_sound::PreviewStatus;
	use std::{path::Path, time::Instant};

	let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/cinder-oath/assets");
	let audio = bridge::project_audio::load(&assets).unwrap().unwrap();
	let mut app = GameApp::default();
	app.project = Some(
		runtime::project::ProjectData::from_json(include_str!(
			"../../../../projects/cinder-oath/project.json"
		))
		.unwrap(),
	);
	app.project_sound = ProjectSound::new(Some(audio), false);
	assert_eq!(app.project_sound.configured_cues(), CombatCue::ALL);
	app.player_controls.applied.audio.muted = true;
	let applied = app.player_controls.applied.clone();
	let now = Instant::now();
	app.feedback_captions.push(CombatCue::Victory, now);
	let caption_expiry = app.feedback_captions.next_expiry();
	let focus = |app: &mut GameApp, label| {
		for _ in 0..14 {
			if app.player_controls_rows(1, 80).unwrap()[0] == format!("> {label}") {
				return;
			}
			app.player_controls_key(Key::ArrowDown);
		}
		panic!("Menu row {label} is unavailable");
	};
	app.player_controls_key(Key::Escape);
	focus(&mut app, "Feedback Audio");
	app.player_controls_key(Key::Enter);

	// Change the draft and leave it unapplied. The check uses the saved/effective mute.
	focus(&mut app, "Mute: On");
	app.player_controls_key(Key::Enter);
	focus(&mut app, "Start Audio Check");
	app.player_controls_key(Key::Enter);
	let rows = app.player_controls_rows(12, 80).unwrap();
	assert_eq!(rows[0], "Sword swing (1/5)");
	assert_eq!(rows[1], "  Gain 0% (Muted)");
	app.player_controls_key(Key::Enter);
	assert_eq!(app.project_sound.preview_status(), PreviewStatus::SilentMix);
	focus(&mut app, "Inaudible");
	app.player_controls_key(Key::Enter);
	app.player_controls_key(Key::Enter);
	assert_eq!(
		app.player_controls_rows(12, 80).unwrap()[0],
		"Sword impact (2/5)"
	);
	assert_eq!(app.project_sound.preview_status(), PreviewStatus::Ready);
	app.player_controls_key(Key::Escape);
	assert_eq!(
		app.player_controls_rows(1, 80).unwrap(),
		["> Start Audio Check"]
	);
	assert_eq!(app.player_controls.applied, applied);
	assert_eq!(app.feedback_captions.next_expiry(), caption_expiry);
	assert_eq!(app.project_sound.preview_status(), PreviewStatus::Ready);
}
