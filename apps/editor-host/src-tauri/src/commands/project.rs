use crate::{
	editor::EditorWorkspace,
	state::{EditorExitState, EditorState},
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use tauri::{Emitter, EventTarget, Manager, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

const WINDOW_CLOSE_REQUESTED: &str = "editor://window-close-requested";

enum ExitRequest {
	FileMenu { expected_revision: Option<u64> },
	WindowClose,
}

impl ExitRequest {
	fn reviewed_revision(&self, workspace: Option<&EditorWorkspace>) -> Option<u64> {
		match self {
			Self::FileMenu { expected_revision } => *expected_revision,
			Self::WindowClose => workspace.map(|workspace| workspace.backend.session().revision()),
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
struct ExitReview {
	generation: Option<u64>,
	revision: Option<u64>,
	timeline_revisions: Vec<(String, u64)>,
	prop_revisions: Vec<(String, u64)>,
	unsaved_changes: Option<&'static str>,
}

impl ExitReview {
	fn capture(
		workspace: Option<&EditorWorkspace>,
		expected_revision: Option<u64>,
	) -> Result<Self, String> {
		let session = workspace.map(|workspace| workspace.backend.session());
		let revision = session.map(|session| session.revision());
		if revision != expected_revision {
			return Err("Revision conflict: the open project changed. Try Exit again.".into());
		}
		if session.is_some_and(|session| session.has_transaction()) {
			return Err("Finish the current edit before exiting.".into());
		}
		let scene_dirty = session.is_some_and(|session| session.is_dirty());
		let timeline_dirty = workspace.is_some_and(EditorWorkspace::has_dirty_clip_timelines);
		let props_dirty = workspace.is_some_and(EditorWorkspace::has_dirty_prop_sources);
		let unsaved_changes = match (scene_dirty, timeline_dirty, props_dirty) {
			(false, false, false) => None,
			(true, false, false) => Some("scene and project changes"),
			(false, true, false) => Some("Animation Clip Timeline changes"),
			(false, false, true) => Some("Prop source changes"),
			(true, true, false) => Some("scene, project, and Animation Clip Timeline changes"),
			(true, false, true) => Some("scene, project, and Prop source changes"),
			(false, true, true) => Some("Animation Clip Timeline and Prop source changes"),
			(true, true, true) => {
				Some("scene, project, Animation Clip Timeline, and Prop source changes")
			}
		};
		Ok(Self {
			generation: workspace.map(|workspace| workspace.generation),
			revision,
			timeline_revisions: workspace
				.map(|workspace| workspace.clip_timeline_drafts.revisions())
				.unwrap_or_default(),
			prop_revisions: workspace
				.map(EditorWorkspace::prop_source_draft_revisions)
				.unwrap_or_default(),
			unsaved_changes,
		})
	}

	fn verify(&self, workspace: Option<&EditorWorkspace>) -> Result<(), String> {
		if Self::capture(workspace, self.revision)? != *self {
			return Err(
				"The project or an open draft changed during review. Try Exit again.".into(),
			);
		}
		Ok(())
	}

	fn retire(
		&self,
		workspace: &mut Option<EditorWorkspace>,
		exit: &EditorExitState,
	) -> Result<Option<EditorWorkspace>, String> {
		self.verify(workspace.as_ref())?;
		exit.commit();
		Ok(workspace.take())
	}
}

#[tauri::command]
pub async fn choose_project_manifest(
	window: tauri::WebviewWindow,
) -> Result<Option<String>, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Open project.json")
			.add_filter("Project manifest", &["json"])
			.set_file_name("project.json")
			.blocking_pick_file()
			.map(|file| {
				file.into_path()
					.map(|path| path.to_string_lossy().into_owned())
					.map_err(|error| error.to_string())
			})
			.transpose()
	})
	.await
	.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn confirm_project_discard(window: tauri::WebviewWindow) -> Result<bool, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		confirm_discard(
			&window,
			"Opening another project will discard all unsaved scene and project changes.",
			"Discard and open",
			"Keep current project",
		)
	})
	.await
	.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn editor_exit(
	window: WebviewWindow,
	expected_revision: Option<u64>,
) -> Result<bool, String> {
	super::editor::trusted_window(&window)?;
	request_editor_exit(window, ExitRequest::FileMenu { expected_revision }).await
}

#[tauri::command]
pub fn editor_set_window_close_review(
	window: WebviewWindow,
	owner: String,
	enabled: bool,
) -> Result<(), String> {
	super::editor::trusted_window(&window)?;
	window
		.state::<EditorState>()
		.exit
		.set_window_close_owner(owner, enabled)
}

pub(crate) fn request_window_close(window: WebviewWindow) {
	tauri::async_runtime::spawn(async move {
		if let Err(message) = dispatch_window_close(&window).await {
			engine_logger::warn!(target: "editor", "Window close kept the editor open: {message}");
			window
				.dialog()
				.message(message)
				.title("Editor remains open")
				.parent(&window)
				.kind(MessageDialogKind::Info)
				.show(|_| {});
		}
	});
}

async fn dispatch_window_close(window: &WebviewWindow) -> Result<(), String> {
	let exit = &window.state::<EditorState>().exit;
	if exit.is_committed() {
		return Ok(());
	}
	let owner = exit.window_close_owner()?.clone();
	if let Some(owner) = owner {
		return window
			.emit_to(
				EventTarget::webview_window(window.label()),
				WINDOW_CLOSE_REQUESTED,
				owner,
			)
			.map_err(|error| format!("The editor could not request a close review: {error}"));
	}
	request_editor_exit(window.clone(), ExitRequest::WindowClose).await?;
	Ok(())
}

async fn request_editor_exit(window: WebviewWindow, request: ExitRequest) -> Result<bool, String> {
	let state = window.state::<EditorState>();
	let editor = Arc::clone(&state.editor);
	let viewport = Arc::clone(&state.viewport);
	let exit = Arc::clone(&state.exit);
	tauri::async_runtime::spawn_blocking(move || {
		let Some(_permit) = exit_review_permit(&exit.review_gate)? else {
			return Ok(false);
		};
		let review = {
			let workspace = editor.lock().map_err(|_| "Editor state lock failed")?;
			if exit.is_committed() {
				return Ok(false);
			}
			let _unclaimed = hold_unclaimed_window_close(&exit, &request)?;
			let expected_revision = request.reviewed_revision(workspace.as_ref());
			ExitReview::capture(workspace.as_ref(), expected_revision)?
		};
		if let Some(changes) = review.unsaved_changes {
			let message = format!("Exiting the editor will discard unsaved {changes}.");
			if !confirm_discard(&window, &message, "Discard and exit", "Keep editing") {
				return Ok(false);
			}
		}

		let mut host = viewport.lock().map_err(|_| "Viewport state lock failed")?;
		let mut workspace = editor.lock().map_err(|_| "Editor state lock failed")?;
		// Close the slot to late edits and prepared project opens at the same time.
		let retired = {
			let _unclaimed = hold_unclaimed_window_close(&exit, &request)?;
			review.retire(&mut workspace, &exit)?
		};
		drop(workspace);
		host.close_parent_when_ready(window);
		drop(host);
		drop(retired);
		Ok(true)
	})
	.await
	.map_err(|error| error.to_string())?
}

fn hold_unclaimed_window_close<'a>(
	exit: &'a EditorExitState,
	request: &ExitRequest,
) -> Result<Option<MutexGuard<'a, Option<String>>>, String> {
	if !matches!(request, ExitRequest::WindowClose) {
		return Ok(None);
	}
	let owner = exit.window_close_owner()?;
	if owner.is_some() {
		return Err("The editor is ready to review changes. Request Close again.".into());
	}
	Ok(Some(owner))
}

fn exit_review_permit(gate: &Mutex<()>) -> Result<Option<MutexGuard<'_, ()>>, String> {
	match gate.try_lock() {
		Ok(guard) => Ok(Some(guard)),
		Err(TryLockError::WouldBlock) => Ok(None),
		Err(TryLockError::Poisoned(_)) => Err("Exit review state lock failed".into()),
	}
}

fn confirm_discard(
	window: &tauri::WebviewWindow,
	message: &str,
	accept: &str,
	cancel: &str,
) -> bool {
	window
		.dialog()
		.message(message)
		.title("Discard unsaved changes?")
		.parent(window)
		.kind(MessageDialogKind::Warning)
		.buttons(MessageDialogButtons::OkCancelCustom(
			accept.into(),
			cancel.into(),
		))
		.blocking_show()
}

#[tauri::command]
pub async fn choose_project_destination(
	window: tauri::WebviewWindow,
	initial_directory: Option<String>,
) -> Result<Option<String>, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		let mut dialog = window
			.dialog()
			.file()
			.set_parent(&window)
			.set_title("Choose a project destination folder");
		if let Some(directory) = initial_directory {
			let path = PathBuf::from(directory.trim());
			if path.is_absolute() && path.is_dir() {
				dialog = dialog.set_directory(path);
			}
		}
		dialog
			.blocking_pick_folder()
			.map(|folder| {
				folder
					.into_path()
					.map(|path| path.to_string_lossy().into_owned())
					.map_err(|error| error.to_string())
			})
			.transpose()
	})
	.await
	.map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
	path: String,
	manifest_path: String,
}

#[tauri::command]
pub async fn create_project(
	window: tauri::WebviewWindow,
	name: String,
	destination: String,
) -> Result<ProjectResult, String> {
	super::editor::trusted_window(&window)?;
	tauri::async_runtime::spawn_blocking(move || {
		let path = compiler::create_authoring_project(&name, &destination)?;
		Ok(ProjectResult {
			manifest_path: path.join("project.json").to_string_lossy().into_owned(),
			path: path.to_string_lossy().into_owned(),
		})
	})
	.await
	.map_err(|error| error.to_string())?
}

#[cfg(test)]
#[path = "../../tests/unit/commands/tests_project.rs"]
mod tests;
