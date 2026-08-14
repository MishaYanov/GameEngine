use std::sync::Mutex;

use runtime::Engine;

pub struct EditorState {
	pub engine: Mutex<Engine>,
}

impl EditorState {
	pub fn new(engine: Engine) -> Self {
		Self {
			engine: Mutex::new(engine),
		}
	}
}