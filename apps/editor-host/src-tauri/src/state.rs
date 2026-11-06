use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bridge::EngineStatusDto;

pub struct PendingProjectBuild {
	pub id: u64,
	pub staged: compiler::StagedProject,
}

#[derive(Default)]
pub struct ProjectBuildState {
	pub latest_id: u64,
	pub pending: Option<PendingProjectBuild>,
}

pub struct EditorState {
	pub engine_status: Arc<Mutex<Option<EngineStatusDto>>>,
	pub editor: Arc<Mutex<Option<crate::editor::EditorWorkspace>>>,
	pub viewport: Arc<Mutex<crate::viewport::ViewportHost>>,
	pub last_build: Arc<Mutex<Option<PathBuf>>>,
	pub pending_build: Arc<Mutex<ProjectBuildState>>,
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
			last_build: Arc::new(Mutex::new(None)),
			pending_build: Arc::new(Mutex::new(ProjectBuildState::default())),
		}
	}
}
