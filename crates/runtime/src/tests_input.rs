use super::*;

const KEYS: [Key; 25] = [
    Key::W,
    Key::A,
    Key::S,
    Key::D,
    Key::Space,
    Key::Escape,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::Tab,
    Key::F5,
    Key::F9,
	Key::F6,
	Key::Q,
	Key::E,
	Key::R,
	Key::F,
	Key::N,
	Key::C,
	Key::Z,
	Key::Y,
	Key::ControlLeft,
	Key::ControlRight,
	Key::Delete,
];

fn assert_state(input: &InputState, key: Key, held: bool, pressed: bool, released: bool) {
    assert_eq!(
        (
            input.is_held(key),
            input.just_pressed(key),
            input.just_released(key)
        ),
        (held, pressed, released),
        "unexpected state for {key:?}",
    );
}

#[test]
fn default_state_has_no_held_keys_or_transitions() {
    let input = InputState::default();
    for key in KEYS {
        assert_state(&input, key, false, false, false);
    }
}

#[test]
fn key_lifecycle_keeps_held_state_but_clears_frame_transitions() {
    for key in KEYS {
        let mut input = InputState::default();
        input.press(key);
        assert_state(&input, key, true, true, false);
        input.end_frame();
        assert_state(&input, key, true, false, false);
        input.release(key);
        assert_state(&input, key, false, false, true);
        input.end_frame();
        assert_state(&input, key, false, false, false);
    }
}

#[test]
fn repeated_key_down_does_not_create_a_new_press_in_later_frames() {
    let mut input = InputState::default();
    input.press(Key::W);
    input.press(Key::W);
    assert_state(&input, Key::W, true, true, false);
    input.end_frame();
    input.press(Key::W);
    assert_state(&input, Key::W, true, false, false);
}

#[test]
fn releasing_an_unheld_key_does_not_create_a_release() {
    let mut input = InputState::default();
    input.release(Key::Escape);
    assert_state(&input, Key::Escape, false, false, false);
    input.press(Key::Escape);
    input.end_frame();
    input.release(Key::Escape);
    input.release(Key::Escape);
    assert_state(&input, Key::Escape, false, false, true);
    input.end_frame();
    input.release(Key::Escape);
    assert_state(&input, Key::Escape, false, false, false);
}

#[test]
fn press_and_release_in_one_frame_preserve_both_transitions() {
    let mut input = InputState::default();
    input.press(Key::Space);
    input.release(Key::Space);
    assert_state(&input, Key::Space, false, true, true);
    input.end_frame();
    assert_state(&input, Key::Space, false, false, false);
}

#[test]
fn release_and_repress_in_one_frame_end_held_with_both_transitions() {
    let mut input = InputState::default();
    input.press(Key::A);
    input.end_frame();
    input.release(Key::A);
    input.press(Key::A);
    assert_state(&input, Key::A, true, true, true);
    input.end_frame();
    assert_state(&input, Key::A, true, false, false);
}

#[test]
fn keys_are_independent_when_pressed_and_released_together() {
    let mut input = InputState::default();
    for key in KEYS {
        input.press(key);
    }
    input.end_frame();
    for key in &KEYS[..5] {
        input.release(*key);
    }
    for (index, key) in KEYS.into_iter().enumerate() {
        assert_state(&input, key, index >= 5, false, index < 5);
    }
}

#[test]
fn reset_clears_held_and_pending_transitions_and_accepts_fresh_presses() {
    let mut input = InputState::default();
    for key in KEYS {
        input.press(key);
    }
    input.release(Key::W);
    input.reset();
    input.reset();
    for key in KEYS {
        assert_state(&input, key, false, false, false);
        input.press(key);
        assert_state(&input, key, true, true, false);
    }
}

#[test]
fn suppression_clears_motion_and_edges_and_ignores_repeat_until_release() {
	let mut input = InputState::default();
	for key in [Key::F6, Key::W, Key::ControlLeft] {
		input.press(key);
	}
	input.suppress_held();
	for _ in 0..3 {
		for key in [Key::F6, Key::W, Key::ControlLeft] {
			input.press(key);
			assert_state(&input, key, false, false, false);
		}
		input.end_frame();
		input.suppress_held();
	}
	input.release(Key::F6);
	assert_state(&input, Key::F6, false, false, false);
	input.press(Key::F6);
	assert_state(&input, Key::F6, true, true, false);
	input.press(Key::W);
	assert_state(&input, Key::W, false, false, false);
}

#[test]
fn focus_reset_rearms_suppressed_keys_and_suppression_does_not_block_new_keys() {
	let mut input = InputState::default();
	input.press(Key::F6);
	input.suppress_held();
	input.press(Key::D);
	assert_state(&input, Key::D, true, true, false);
	input.reset();
	input.press(Key::F6);
	assert_state(&input, Key::F6, true, true, false);
}
