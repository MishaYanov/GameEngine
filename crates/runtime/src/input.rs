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
    Tab,
    F5,
    F9,
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
#[path = "tests_input.rs"]
mod tests;
