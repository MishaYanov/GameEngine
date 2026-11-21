use super::*;
use runtime::input::GameInputBinding;

#[test]
fn movement_presets_preserve_authored_actions() {
	let mut authored = InputProfile::default();
	authored.actions.push(GameInputBinding {
		id: "interact".into(),
		key: Key::E,
		animation_parameter: Some("use".into()),
	});
	let arrows = Preset::Arrows.resolve(Some(&authored)).unwrap();
	assert_eq!(arrows.movement, MovementBindings::ARROWS);
	assert_eq!(arrows.actions, authored.actions);
	assert_eq!(Preset::Project.resolve(Some(&authored)).unwrap(), authored);
	assert_eq!(Preset::Wasd.resolve(None).unwrap(), InputProfile::default());

	authored.actions[0].key = Key::ArrowLeft;
	assert!(Preset::Arrows.resolve(Some(&authored)).is_err());
	assert!(Preset::Project.resolve(Some(&authored)).is_ok());
}

#[test]
fn escape_cannot_be_assigned_to_a_launch_profile() {
	let mut profile = InputProfile::default();
	profile.movement.positive_x = Key::Escape;
	assert!(validate_profile(&profile).is_err());
}

#[test]
fn controls_owns_keyboard_until_cancel() {
	let mut app = GameApp::default();
	app.input.press(Key::W);
	assert!(app.player_controls_key(Key::Escape));
	assert!(app.player_controls.is_open());
	assert!(!app.input.is_held(Key::W));
	assert!(app.player_controls_key(Key::ArrowRight));
	assert!(app.player_controls.rows().unwrap()[1].starts_with("> Movement: Arrow"));
	assert!(app.player_controls_key(Key::Escape));
	assert!(!app.player_controls.is_open());
	assert_eq!(app.player_controls.applied, Preset::Project);
	app.input.press(Key::W);
	assert!(!app.input.is_held(Key::W));
	app.input.release(Key::W);
	app.input.press(Key::W);
	assert!(app.input.is_held(Key::W));
}

#[test]
fn command_line_override_cannot_be_changed_in_controls() {
	let mut app = GameApp::default();
	assert!(
		app.player_controls
			.configure(None, None, None, true)
			.is_none()
	);
	app.player_controls_key(Key::Escape);
	app.player_controls_key(Key::ArrowRight);
	app.player_controls_key(Key::ArrowDown);
	app.player_controls_key(Key::Enter);
	assert!(app.player_controls.is_open());
	assert!(app.player_controls.rows().unwrap()[2].contains("Apply disabled"));
}

#[test]
fn continue_actions_have_stable_availability_labels() {
	use ContinueAvailability::*;

	for (availability, label, enabled) in [
		(Empty, "No saved checkpoint", false),
		(Current, "Ready", true),
		(CompatibleEarlierRelease, "Earlier release", true),
		(IncompatibleRelease, "Incompatible with this release", false),
		(Unreadable, "Unreadable save", false),
	] {
		assert_eq!(continue_label(availability), label);
		assert_eq!(availability.can_continue(), enabled);
	}
}

#[test]
fn controller_draft_is_cancelled_and_focus_stays_visible_in_a_short_menu() {
	let mut app = GameApp::default();
	for key in [
		Key::Escape,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::Enter,
		Key::ArrowDown,
		Key::ArrowLeft,
	] {
		assert!(app.player_controls_key(key));
	}
	assert_eq!(
		app.player_controls.rows_with_capacity(2).unwrap()[0],
		"> Interact: B"
	);
	assert_eq!(app.player_controls.controller().interact, FaceButton::X);
	app.player_controls_key(Key::Escape);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.iter()
			.any(|row| row == "> Controller")
	);
	assert_eq!(app.player_controls.controller().interact, FaceButton::X);
}

#[test]
fn gate_labels_follow_the_device_and_only_the_bound_logical_action() {
	let mut profile = InputProfile::default();
	profile.actions.push(GameInputBinding {
		id: "interact".into(),
		key: Key::E,
		animation_parameter: None,
	});
	let map = ControllerBindings {
		jump: FaceButton::A,
		interact: FaceButton::B,
	};
	let hint = |device| {
		crate::scene_rendering::interact_hint(Some(&profile), Some("interact"), map, device)
	};
	assert!(hint(PromptDevice::Controller).starts_with("Press B"));
	assert!(hint(PromptDevice::Keyboard).starts_with("Press E"));
	assert!(
		crate::scene_rendering::interact_hint(
			None,
			Some("interact"),
			map,
			PromptDevice::Controller
		)
		.contains("unbound")
	);
}
