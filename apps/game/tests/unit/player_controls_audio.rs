use super::*;
use crate::GameApp;

fn focus_row(page: &mut AudioMenu, label: &str) {
	for _ in 0..AudioMenu::ROWS.len() {
		let (_, rows, focus) = page.settings_rows();
		if rows[focus].starts_with(label) {
			return;
		}
		page.key(Key::ArrowDown);
	}
	panic!("Missing audio row {label}");
}

#[test]
fn player_audio_menu_clamps_volume_and_retains_it_beneath_mute() {
	let mut page = AudioMenu::new(FeedbackMix::default());
	assert!(matches!(page.key(Key::ArrowRight), AudioAction::Stay));
	assert_eq!(page.settings_rows().1[0], "Volume: 100%");
	for _ in 0..12 {
		page.key(Key::ArrowLeft);
	}
	assert_eq!(page.settings_rows().1[0], "Volume: 0%");
	for _ in 0..3 {
		page.key(Key::ArrowRight);
	}
	page.key(Key::ArrowDown);
	page.key(Key::Enter);
	assert_eq!(page.settings_rows().1[1], "Mute: On");
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(muted) = page.key(Key::Enter) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(muted.level_percent, 30);
	assert_eq!(muted.effective_percent(), 0);
	focus_row(&mut page, "Mute");
	page.key(Key::ArrowLeft);
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(audible) = page.key(Key::Enter) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(audible.effective_percent(), 30);
}

#[test]
fn restoring_category_defaults_edits_only_the_draft_categories() {
	let mut page = AudioMenu::new(FeedbackMix::default());
	page.key(Key::ArrowLeft);
	focus_row(&mut page, "Mute");
	page.key(Key::Enter);

	for bus in [
		FeedbackBus::Collection,
		FeedbackBus::Mechanisms,
		FeedbackBus::Completion,
	] {
		focus_row(&mut page, bus_label(bus));
		for _ in 0..12 {
			page.key(Key::ArrowLeft);
		}
		assert_eq!(page.draft.category_percent(bus), 0);
	}

	focus_row(&mut page, "Restore Category Defaults");
	assert!(matches!(page.key(Key::Enter), AudioAction::Stay));
	focus_row(&mut page, "Apply");
	let AudioAction::Apply(restored) = page.key(Key::Enter) else {
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
	let mut page = AudioMenu::new(FeedbackMix::default());
	focus_row(&mut page, "Cue Details");
	page.key(Key::Enter);
	let (_, rows, focus) = page.rows(&audio, false);
	assert_eq!(rows[focus], "Category: Collection");
	assert!(rows.contains(&"Clip: Missing".into()));
	assert!(rows.contains(&"Cue captions: Off".into()));

	page.key(Key::ArrowRight);
	let (_, rows, focus) = page.rows(&audio, true);
	assert_eq!(rows[focus], "Category: Mechanisms");
	assert!(rows.contains(&"Master: 0%".into()));
	assert!(rows.contains(&"Resolved: 0%".into()));
	assert!(rows.contains(&"Clip: Built-in".into()));
	assert!(rows.contains(&"Cue captions: On".into()));

	page.key(Key::Escape);
	let (title, rows, focus) = page.rows(&audio, true);
	assert_eq!(title, "Feedback Audio");
	assert_eq!(rows[focus], "Cue Details");
}
