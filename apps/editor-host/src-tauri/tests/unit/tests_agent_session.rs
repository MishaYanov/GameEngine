use super::*;
use authoring_agent::session::RequestScope;
use std::{fs, path::PathBuf};

#[test]
fn only_a_matching_request_can_stop_or_release_the_foreground_worker() {
	let sessions = AgentSessions::default();
	let old = sessions.reserve("old", 3).ok().unwrap();
	sessions.stop("old", 2);
	sessions.stop("new", 3);
	assert!(!old.is_cancelled());
	assert!(matches!(
		sessions.reserve("new", 3),
		Err(ReserveError::Busy)
	));

	let new = sessions.reserve("new", 4).ok().unwrap();
	assert!(old.is_cancelled());
	assert_eq!(sessions.release(&old), None);
	assert!(sessions.owns(&new));
	sessions.stop("new", 4);
	assert_eq!(sessions.release(&new), Some(true));
}

#[test]
fn a_new_window_document_retires_the_existing_worker_lease() {
	let sessions = AgentSessions::default();
	let old = sessions.reserve("same-id", 5).ok().unwrap();
	sessions.invalidate();
	assert!(old.is_cancelled());
	assert!(!sessions.page_is_current(&old));
	let next = sessions.reserve("same-id", 5).ok().unwrap();
	assert!(sessions.page_is_current(&next));
	assert_eq!(sessions.release(&old), None);
	assert_eq!(sessions.release(&next), Some(false));
}

#[test]
fn staged_agent_replies_with_scope_and_drops_late_results_on_stop_or_switch() {
	// The release smoke check points this at the actual D: staged worker.
	let Some(root) = std::env::var_os("ENGINE_AGENT_TEST_RESOURCES").map(PathBuf::from) else {
		return;
	};
	let help = fs::metadata(root.join("engine-help")).unwrap();
	assert!(help.is_dir());

	for outcome in ["retrieval_only", "cancelled", "retired"] {
		let resources = AgentResources {
			executable: root.join("authoring-agent-worker.exe"),
			root: root.clone(),
		};
		let sessions = AgentSessions::default();
		let ticket = sessions.reserve("fixture", 7).ok().unwrap();
		let request = AgentRequest {
			id: "fixture".into(),
			scope: RequestScope {
				project_id: "fixture-project".into(),
				workspace_generation: 7,
				revision: 2,
			},
			question: "camera movement".into(),
			selection: None,
		};
		let foreground = AtomicBool::new(true);
		let events = Mutex::new(Vec::<Value>::new());
		run_agent_worker(
			&sessions,
			&ticket,
			resources,
			None,
			request,
			|| foreground.load(Ordering::Acquire),
			|event| {
				assert_eq!(event["workspace_generation"], 7);
				assert_eq!(event["id"], "fixture");
				if event["type"] == "started" {
					match outcome {
						"cancelled" => sessions.stop("fixture", 7),
						"retired" => foreground.store(false, Ordering::Release),
						_ => {}
					}
				}
				events.lock().unwrap().push(event);
				true
			},
		);
		let events = events.into_inner().unwrap();
		assert_eq!(events[0]["type"], "started");
		match outcome {
			"retired" => assert_eq!(events.len(), 1),
			"cancelled" => {
				assert_eq!(events.len(), 2);
				assert_eq!(events[1]["status"], "cancelled");
			}
			_ => {
				assert_eq!(events.len(), 3);
				assert_ne!(events[1]["citations"].as_array().unwrap().len(), 0);
				assert_eq!(events[2]["status"], "retrieval_only");
			}
		}
		assert!(!sessions.owns(&ticket));
	}
}
