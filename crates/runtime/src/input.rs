use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    Space,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Default)]
pub struct InputState {
    held: HashSet<Key>,
    pressed: HashSet<Key>,
    released: HashSet<Key>,
}

impl InputState {
    pub fn press(&mut self, key: Key) {
        if self.held.insert(key) {
            self.pressed.insert(key);
        }
    }

    pub fn release(&mut self, key: Key) {
        if self.held.remove(&key) {
            self.released.insert(key);
        }
    }

    pub fn is_held(&self, key: Key) -> bool {
        self.held.contains(&key)
    }

    pub fn just_pressed(&self, key: Key) -> bool {
        self.pressed.contains(&key)
    }

    pub fn just_released(&self, key: Key) -> bool {
        self.released.contains(&key)
    }

    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
    }
    pub fn reset(&mut self) {
        self.held.clear();
        self.pressed.clear();
        self.released.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [Key; 10] = [
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
}
