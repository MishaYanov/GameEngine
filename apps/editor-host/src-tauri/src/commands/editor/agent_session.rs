//! Trusted editor IPC for supervision of the separate authoring Agent worker.

use super::*;
use crate::{
	agent_session::{run_agent_worker, ReserveError, AGENT_SESSION_EVENT},
	editor::agent_context::{
		AgentContextError, AgentContextErrorCode, AgentContextRequest, InspectorAddress,
	},
	release_resources::AgentResources,
};
use authoring_agent::session::{
	AgentRequest, RequestScope, SelectionSnapshot, MAX_REQUEST_ID_BYTES,
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionSelection {
	address: InspectorAddress,
	content_revision: u64,
	component_id: Option<runtime::components::ComponentId>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionStart {
	id: String,
	workspace_generation: u64,
	revision: u64,
	question: String,
	selection: Option<AgentSessionSelection>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionStop {
	id: String,
	workspace_generation: u64,
}

#[tauri::command]
pub async fn editor_agent_session_start(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: AgentSessionStart,
) -> Result<(), AgentContextError> {
	verify_window(&window)?;
	if !valid_request_id(&request.id)
		|| request.question.trim().is_empty()
		|| request.question.chars().count() > 512
	{
		return Err(AgentContextError::new(
			AgentContextErrorCode::Unsupported,
			"The Agent request ID or question is invalid",
		));
	}
	let shared = Arc::clone(&state.editor);
	let sessions = Arc::clone(&state.agent_sessions);
	tauri::async_runtime::spawn_blocking(move || {
		use AgentContextErrorCode::{Missing, Stale, Unsupported};

		let generation = request.workspace_generation;
		let (project_root, captured) = {
			let slot = shared.lock().map_err(|_| unavailable())?;
			let workspace = slot.as_ref().ok_or_else(|| {
				AgentContextError::new(Missing, "Open a project before asking the Agent")
			})?;
			if workspace.generation != generation
				|| workspace.backend.session().revision() != request.revision
			{
				return Err(AgentContextError::new(
					Stale,
					"The authoring workspace changed",
				));
			}
			let captured = request
				.selection
				.map(|selection| {
					workspace.capture_agent_context(AgentContextRequest {
						workspace_generation: generation,
						revision: request.revision,
						content_revision: selection.content_revision,
						address: selection.address,
						component_id: selection.component_id,
					})
				})
				.transpose()?;
			(
				workspace.project_path.parent().map(Path::to_path_buf),
				captured,
			)
		};
		let resources = AgentResources::for_window(&window).map_err(|_| {
			AgentContextError::new(
				AgentContextErrorCode::Unavailable,
				"The bundled Agent worker or help is unavailable",
			)
		})?;
		let selection = captured
			.map(|capture| {
				let summary = capture.resolve()?;
				serde_json::to_value(summary)
					.and_then(serde_json::from_value::<SelectionSnapshot>)
					.map_err(|_| {
						AgentContextError::new(
							Unsupported,
							"The captured selection uses an unsupported Agent schema",
						)
					})
			})
			.transpose()?;
		let ticket = {
			let slot = shared.lock().map_err(|_| unavailable())?;
			if !slot.as_ref().is_some_and(|workspace| {
				workspace.generation == generation
					&& workspace.backend.session().revision() == request.revision
			}) {
				return Err(AgentContextError::new(
					Stale,
					"The authoring workspace changed",
				));
			}
			sessions
				.reserve(&request.id, generation)
				.map_err(|error| match error {
					ReserveError::Busy => AgentContextError::new(
						Unsupported,
						"Stop the active answer before asking another question",
					),
					ReserveError::Unavailable => unavailable(),
				})?
		};
		let worker_request = AgentRequest {
			id: request.id,
			scope: RequestScope {
				project_id: format!("workspace-{generation}"),
				workspace_generation: generation,
				revision: request.revision,
			},
			question: request.question,
			selection,
		};
		let worker_sessions = Arc::clone(&sessions);
		let worker_ticket = ticket.clone();
		let window_label = window.label().to_owned();
		std::thread::Builder::new()
			.name("editor-agent-worker".into())
			.spawn(move || {
				run_agent_worker(
					&worker_sessions,
					&worker_ticket,
					resources,
					project_root.as_deref(),
					worker_request,
					|| {
						shared.lock().is_ok_and(|slot| {
							slot.as_ref()
								.is_some_and(|workspace| workspace.generation == generation)
						})
					},
					|event| {
						window
							.emit_to(window_label.as_str(), AGENT_SESSION_EVENT, event)
							.is_ok()
					},
				);
			})
			.map_err(|_| {
				let _ = sessions.release(&ticket);
				unavailable()
			})?;
		Ok(())
	})
	.await
	.map_err(|_| unavailable())?
}

#[tauri::command]
pub fn editor_agent_session_stop(
	window: WebviewWindow,
	state: State<'_, EditorState>,
	request: AgentSessionStop,
) -> Result<(), AgentContextError> {
	verify_window(&window)?;
	if valid_request_id(&request.id) {
		state
			.agent_sessions
			.stop(&request.id, request.workspace_generation);
	}
	Ok(())
}

fn valid_request_id(id: &str) -> bool {
	!id.is_empty()
		&& id.len() <= MAX_REQUEST_ID_BYTES
		&& id
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn verify_window(window: &WebviewWindow) -> Result<(), AgentContextError> {
	trusted_window(window).map_err(|_| {
		AgentContextError::new(
			AgentContextErrorCode::Forbidden,
			"The Agent requires the trusted Editor window",
		)
	})
}

fn unavailable() -> AgentContextError {
	AgentContextError::new(
		AgentContextErrorCode::Unavailable,
		"The Agent service is unavailable",
	)
}
