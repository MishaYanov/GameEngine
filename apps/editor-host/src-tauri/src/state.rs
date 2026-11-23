use std::sync::{
	atomic::{AtomicBool, Ordering},
	Arc, Mutex, MutexGuard,
};

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

#[derive(Default)]
pub(crate) struct EditorExitState {
	pub(crate) review_gate: Mutex<()>,
	committed: AtomicBool,
	window_close_owner: Mutex<Option<String>>,
}

impl EditorExitState {
	pub(crate) fn window_close_owner(&self) -> Result<MutexGuard<'_, Option<String>>, String> {
		self.window_close_owner
			.lock()
			.map_err(|_| "Window close review state lock failed".into())
	}

	pub(crate) fn set_window_close_owner(
		&self,
		owner: String,
		enabled: bool,
	) -> Result<(), String> {
		if owner.is_empty()
			|| owner.len() > 64
			|| !owner
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
		{
			return Err("Invalid window close review owner.".into());
		}
		let mut current = self.window_close_owner()?;
		if enabled {
			self.ensure_project_can_open()?;
			*current = Some(owner);
		} else if current.as_deref() == Some(owner.as_str()) {
			*current = None;
		}
		Ok(())
	}

	pub(crate) fn is_committed(&self) -> bool {
		self.committed.load(Ordering::Relaxed)
	}

	/// Call with the workspace slot locked while retiring the reviewed workspace.
	pub(crate) fn commit(&self) {
		self.committed.store(true, Ordering::Relaxed);
	}

	/// Check under the same workspace lock before adopting a prepared project.
	pub(crate) fn ensure_project_can_open(&self) -> Result<(), String> {
		if self.is_committed() {
			return Err("The editor is closing.".into());
		}
		Ok(())
	}
}

pub struct EditorState {
	pub engine_status: Arc<Mutex<Option<EngineStatusDto>>>,
	pub editor: Arc<Mutex<Option<crate::editor::EditorWorkspace>>>,
	pub viewport: Arc<Mutex<crate::viewport::ViewportHost>>,
	pub(crate) exit: Arc<EditorExitState>,
	pub last_build: Arc<Mutex<Option<compiler::InstalledProject>>>,
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
			exit: Arc::new(EditorExitState::default()),
			last_build: Arc::new(Mutex::new(None)),
			pending_build: Arc::new(Mutex::new(ProjectBuildState::default())),
		}
	}
}
