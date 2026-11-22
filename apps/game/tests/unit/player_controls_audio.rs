use super::*;
use crate::GameApp;

#[test]
fn player_audio_menu_clamps_volume_and_retains_it_beneath_mute() {
	let mut page = AudioMenu::new(FeedbackMix::default());
	assert!(matches!(page.key(Key::ArrowRight), AudioAction::Stay));
	assert_eq!(page.rows().1[0], "Volume: 100%");
	for _ in 0..12 {
		page.key(Key::ArrowLeft);
	}
	assert_eq!(page.rows().1[0], "Volume: 0%");
	for _ in 0..3 {
		page.key(Key::ArrowRight);
	}
	page.key(Key::ArrowDown);
	page.key(Key::Enter);
	assert_eq!(page.rows().1[1], "Mute: On");
	page.key(Key::ArrowDown);
	let AudioAction::Apply(muted) = page.key(Key::Enter) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(muted.level_percent, 30);
	assert_eq!(muted.effective_percent(), 0);
	page.key(Key::ArrowUp);
	page.key(Key::ArrowLeft);
	page.key(Key::ArrowDown);
	let AudioAction::Apply(audible) = page.key(Key::Enter) else {
		panic!("Apply must return the edited settings");
	};
	assert_eq!(audible.effective_percent(), 30);
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
	for key in [Key::ArrowLeft, Key::ArrowDown, Key::ArrowDown, Key::Enter] {
		app.player_controls_key(key);
	}
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
		MenuButtons::default(),
		[false, true, false, false],
	);
	assert_eq!(app.player_controls.feedback_mix(), FeedbackMix::default());
}
