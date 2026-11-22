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
	assert_eq!(app.player_controls.applied.movement, Preset::Project);
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

#[cfg(windows)]
#[test]
fn menu_help_follows_accepted_input_and_preserves_the_focus_marker() {
	use runtime::input::MenuButtons;
	let mut app = GameApp::default();
	let released = PhysicalGamepadSample::default();
	let start = PhysicalGamepadSample {
		menu: MenuButtons {
			open: true,
			..MenuButtons::default()
		},
		..released
	};
	assert!(!app.player_controls_gamepad(released));
	assert!(app.player_controls_gamepad(start));
	assert!(app.player_controls.rows_with_capacity(2).unwrap()[1].starts_with("D-pad"));
	assert!(app.player_controls_key(Key::ArrowUp));
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> Back"]
	);
	assert!(app.player_controls.rows_with_capacity(2).unwrap()[1].starts_with("Arrows"));
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

#[test]
fn accessibility_preview_is_discarded_and_failed_apply_preserves_live_settings() {
	let mut app = GameApp::default();
	for key in [
		Key::Escape,
		Key::ArrowUp,
		Key::ArrowUp,
		Key::Enter,
		Key::ArrowRight,
	] {
		assert!(app.player_controls_key(key));
	}
	assert_eq!(
		app.player_controls.rows_with_layout(2, 25).unwrap()[0],
		"> Text size: Large"
	);
	assert_eq!(app.player_controls.display(), DisplaySettings::default());
	assert!(app.player_controls_key(Key::Escape));
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.iter()
			.any(|row| row == "> Accessibility")
	);
	app.player_controls_key(Key::Enter);
	assert_eq!(
		app.player_controls.rows_with_layout(2, 25).unwrap()[0],
		"> Text size: Standard"
	);
	for key in [
		Key::ArrowRight,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::Enter,
	] {
		app.player_controls_key(key);
	}
	let rows = app.player_controls.rows_with_layout(2, 40).unwrap();
	assert_eq!(rows[0], "> Apply");
	assert!(rows[1].contains("storage"));
	assert_eq!(app.player_controls.display(), DisplaySettings::default());
}

#[cfg(windows)]
#[test]
fn controller_edits_accessibility_and_short_help_matches_accepted_device() {
	use runtime::input::MenuButtons;
	let mut app = GameApp::default();
	let neutral = PhysicalGamepadSample::default();
	let press = |app: &mut GameApp, menu, face_buttons| {
		app.player_controls_gamepad(neutral);
		assert!(app.player_controls_gamepad(PhysicalGamepadSample {
			menu,
			face_buttons,
			..neutral
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
	for _ in 0..2 {
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
			right: true,
			..MenuButtons::default()
		},
		[false; 4],
	);
	assert_eq!(
		app.player_controls.rows_with_layout(2, 22).unwrap(),
		["> Text size: Large", "A: select  B: back"]
	);
	press(
		&mut app,
		MenuButtons::default(),
		[false, true, false, false],
	);
	assert_eq!(app.player_controls.display(), DisplaySettings::default());
	assert_eq!(
		app.player_controls.rows_with_layout(1, 22).unwrap(),
		["> Accessibility"]
	);
}

fn authored_keyboard_fixture() -> InputProfile {
	InputProfile {
		actions: vec![
			GameInputBinding {
				id: "interact".into(),
				key: Key::E,
				animation_parameter: Some("use".into()),
			},
			GameInputBinding {
				id: "jump".into(),
				key: Key::Space,
				animation_parameter: Some("jumping".into()),
			},
		],
		..InputProfile::default()
	}
}

fn override_key(action_id: &str, key: Key) -> ActionKeyOverride {
	ActionKeyOverride {
		action_id: action_id.into(),
		key,
	}
}

#[test]
fn composed_keyboard_remap_checks_final_collisions_and_preserves_metadata() {
	let mut authored = authored_keyboard_fixture();
	let overrides = [
		override_key("interact", Key::Space),
		override_key("jump", Key::E),
	];
	let resolved = action_keys::resolve(&authored, MovementBindings::ARROWS, &overrides).unwrap();
	assert_eq!(resolved.movement, MovementBindings::ARROWS);
	assert_eq!(resolved.actions[0].key, Key::Space);
	assert_eq!(resolved.actions[1].key, Key::E);
	for (actual, old) in resolved.actions.iter().zip(&authored.actions) {
		assert_eq!(actual.id, old.id);
		assert_eq!(actual.animation_parameter, old.animation_parameter);
	}
	for invalid in [
		vec![override_key("interact", Key::ArrowRight)],
		vec![override_key("interact", Key::Space)],
		vec![
			override_key("interact", Key::F),
			override_key("interact", Key::J),
		],
		vec![override_key("removed", Key::Q)],
		vec![override_key("interact", Key::Escape)],
		vec![override_key(
			"interact",
			Key::Unidentified(runtime::NativeKeyCode::Unidentified),
		)],
	] {
		assert!(action_keys::resolve(&authored, MovementBindings::ARROWS, &invalid).is_err());
	}
	authored.actions[0].key = Key::ArrowRight;
	assert!(Preset::Arrows.resolve(Some(&authored)).is_err());
	assert!(
		action_keys::resolve(
			&authored,
			MovementBindings::ARROWS,
			&[override_key("interact", Key::F)],
		)
			.is_ok()
	);
}

#[test]
fn stale_overrides_keep_settings_available_while_using_authored_keys() {
	let authored = authored_keyboard_fixture();
	let settings = ControlsSettings {
		movement: Preset::Arrows,
		actions: vec![override_key("removed", Key::Q)],
		..ControlsSettings::default()
	};
	let mut controls = PlayerControls::default();
	assert!(
		controls
			.restore_settings(settings.clone(), Some(&authored))
			.is_none()
	);
	assert_eq!(controls.applied, settings);
	assert!(
		controls
			.keyboard_problem
			.as_deref()
			.unwrap()
			.contains("removed")
	);
	assert!(
		controls
			.message
			.as_deref()
			.unwrap()
			.contains("project defaults")
	);
}

#[test]
fn capture_cancel_conflict_and_short_rows_do_not_change_the_live_binding() {
	let authored = authored_keyboard_fixture();
	let project = runtime::project::ProjectData::from_json(include_str!(
		"../../project.release-learning.json"
	))
		.unwrap();
	let mut app = GameApp::default();
	app.project = Some(project);
	app.project.as_mut().unwrap().input_profile = Some(authored.clone());
	assert!(app.player_controls_key(Key::Escape));
	for _ in 0..12 {
		if app.player_controls.rows_with_capacity(1).unwrap()[0] == "> Keyboard" {
			break;
		}
		assert!(app.player_controls_key(Key::ArrowDown));
	}
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> Keyboard"]
	);
	assert!(app.player_controls_key(Key::Enter));
	assert_eq!(app.player_controls.rows_with_layout(1, 7).unwrap(), ["> E"]);
	assert!(app.player_controls_key(Key::Enter));
	assert_eq!(
		app.player_controls.rows_with_layout(1, 7).unwrap(),
		["> Key?"]
	);
	app.player_controls.cancel_action_key_capture();
	assert!(!app.player_controls.captures_action_key());
	assert_eq!(app.player_controls.rows_with_layout(1, 7).unwrap(), ["> E"]);

	app.player_controls_key(Key::Enter);
	app.player_controls_key(Key::Space);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.last()
			.unwrap()
			.contains("already assigned")
	);
	assert_eq!(app.effective_input_profile().unwrap(), &authored);
	assert!(!app.input.is_held(Key::Space));
	app.input.press(Key::Space);
	assert!(!app.input.is_held(Key::Space));
	app.input.release(Key::Space);
	app.input.press(Key::Space);
	assert!(app.input.is_held(Key::Space));

	app.player_controls_key(Key::ArrowDown);
	app.player_controls_key(Key::Enter);
	app.player_controls_key(Key::E);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.last()
			.unwrap()
			.starts_with("Arrows")
	);
	app.player_controls_key(Key::Escape);
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> Keyboard"]
	);
	assert_eq!(app.effective_input_profile().unwrap(), &authored);
}

#[cfg(windows)]
#[test]
fn controller_starts_and_cancels_keyboard_capture_without_assigning_a_face_button() {
	let mut app = GameApp::default();
	let mut project = runtime::project::ProjectData::from_json(include_str!(
		"../../project.release-learning.json"
	))
		.unwrap();
	project.input_profile = Some(authored_keyboard_fixture());
	app.project = Some(project);
	app.player_controls_key(Key::Escape);
	for _ in 0..12 {
		if app.player_controls.rows_with_capacity(1).unwrap()[0] == "> Keyboard" {
			break;
		}
		assert!(app.player_controls_key(Key::ArrowDown));
	}
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> Keyboard"]
	);
	app.player_controls_key(Key::Enter);
	let neutral = PhysicalGamepadSample::default();
	app.player_controls_gamepad(neutral);
	let a = PhysicalGamepadSample {
		face_buttons: [true, false, false, false],
		..neutral
	};
	assert!(app.player_controls_gamepad(a));
	assert!(app.player_controls.captures_action_key());
	app.player_controls_gamepad(neutral);
	let b = PhysicalGamepadSample {
		face_buttons: [false, true, false, false],
		..neutral
	};
	assert!(app.player_controls_gamepad(b));
	assert!(!app.player_controls.captures_action_key());
	assert_eq!(
		app.player_controls.rows_with_capacity(1).unwrap(),
		["> interact: E"]
	);
	assert_eq!(app.player_controls.menu_device, PromptDevice::Controller);
}
