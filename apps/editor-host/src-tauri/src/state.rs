use std::sync::{
	atomic::{AtomicBool, AtomicU64, Ordering},
	Arc, Mutex, MutexGuard,
};

use bridge::EngineStatusDto;

pub(crate) mod project_build;
use project_build::ProjectBuildState;

#[derive(Default)]
pub(crate) struct WindowCloseReview {
	page: u64,
	owner: Option<String>,
}

impl WindowCloseReview {
	pub(crate) fn owner(&self) -> Option<&str> {
		self.owner.as_deref()
	}
}

#[derive(Default)]
pub(crate) struct EditorExitState {
	pub(crate) review_gate: Mutex<()>,
	committed: AtomicBool,
	window_close_page: AtomicU64,
	window_close_review: Mutex<WindowCloseReview>,
}

impl EditorExitState {
	pub(crate) fn window_close_review(&self) -> Result<MutexGuard<'_, WindowCloseReview>, String> {
		let mut review = self
			.window_close_review
			.lock()
			.map_err(|_| "Window close review state lock failed")?;
		let page = self.window_close_page.load(Ordering::Relaxed);
		if review.page != page {
			*review = WindowCloseReview { page, owner: None };
		}
		Ok(review)
	}

	/// A new WebView document has no editor listener. Invalidate its predecessor on the UI thread.
	pub(crate) fn reset_window_close_page(&self) {
		self.window_close_page.fetch_add(1, Ordering::Relaxed);
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
		let mut current = self.window_close_review()?;
		if enabled {
			self.ensure_project_can_open()?;
			current.owner = Some(owner);
		} else if current.owner() == Some(owner.as_str()) {
			current.owner = None;
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
	pub(crate) agent_sessions: Arc<crate::agent_session::AgentSessions>,
	pub(crate) mcp_access: Arc<crate::mcp_access::Access>,
	pub engine_status: Arc<Mutex<Option<EngineStatusDto>>>,
	pub editor: Arc<Mutex<Option<crate::editor::EditorWorkspace>>>,
	pub viewport: Arc<Mutex<crate::viewport::ViewportHost>>,
	pub(crate) exit: Arc<EditorExitState>,
	pub(crate) project_build: Arc<Mutex<ProjectBuildState>>,
}

impl Default for EditorState {
	fn default() -> Self {
		let editor = Arc::new(Mutex::new(None));
		let viewport = Arc::new(Mutex::new(crate::viewport::ViewportHost::new(Arc::clone(
			&editor,
		))));
		Self {
			agent_sessions: Arc::new(crate::agent_session::AgentSessions::default()),
			mcp_access: Arc::new(crate::mcp_access::Access::default()),
			engine_status: Arc::new(Mutex::new(None)),
			editor,
			viewport,
			exit: Arc::new(EditorExitState::default()),
			project_build: Arc::new(Mutex::new(ProjectBuildState::default())),
		}
	}
}
