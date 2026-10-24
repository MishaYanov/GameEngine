use std::sync::{Arc, Mutex};

use bridge::EngineStatusDto;

pub struct EditorState {
	pub engine_status: Arc<Mutex<Option<EngineStatusDto>>>,
	pub editor: Arc<Mutex<Option<crate::editor::EditorWorkspace>>>,
	pub viewport: Arc<Mutex<crate::viewport::ViewportHost>>,
}

impl Default for EditorState {
	fn default() -> Self {
		let editor = Arc::new(Mutex::new(None));
		let viewport = Arc::new(Mutex::new(crate::viewport::ViewportHost::new(Arc::clone(
			&editor,
		))));
		Self {
			engine_status: Arc::new(Mutex::new(None)),
			editor,
			viewport,
		}
	}
}
