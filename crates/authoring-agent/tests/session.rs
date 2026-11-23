use authoring_agent::{
	knowledge::ReferenceHit,
	session::{
		CancellationToken, FinishStatus, ModelError, ModelInput, ModelProvider, ReferenceProvider,
		Session, WorkerEvent, WorkerEventKind, WorkerInput, WorkerScope, read_frame,
	},
};
use serde_json::{Value, json};
use std::{
	io::{BufReader, Cursor, ErrorKind},
	sync::mpsc::{self, Receiver},
	thread,
	time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(3);

struct NoReferences;
impl ReferenceProvider for NoReferences {
	fn search(&mut self, _: &str, _: &CancellationToken) -> Vec<ReferenceHit> {
		Vec::new()
	}
}

struct Deterministic;
impl ModelProvider for Deterministic {
	fn generate(
		&mut self,
		input: ModelInput<'_>,
		_: &CancellationToken,
		emit: &mut dyn FnMut(&str) -> bool,
	) -> Result<(), ModelError> {
		assert_eq!(input.question, "How is this object animated?");
		let selection = input.selection.unwrap();
		assert_eq!(selection.name, "Repeated label");
		assert_eq!(selection.entity_key, "instance-2");
		assert_eq!(
			selection.selected_component.as_ref().unwrap().id,
			"animator"
		);
		assert!(emit("Use "));
		assert!(emit("the linked Animator."));
		Ok(())
	}
}

struct WaitForStop;
impl ModelProvider for WaitForStop {
	fn generate(
		&mut self,
		_: ModelInput<'_>,
		cancel: &CancellationToken,
		emit: &mut dyn FnMut(&str) -> bool,
	) -> Result<(), ModelError> {
		assert!(emit("first"));
		let deadline = Instant::now() + WAIT;
		while !cancel.is_cancelled() && Instant::now() < deadline {
			thread::sleep(Duration::from_millis(5));
		}
		assert!(cancel.is_cancelled());
		assert!(!emit("late"));
		Err(ModelError::Interrupted)
	}
}

struct Oversized;
impl ModelProvider for Oversized {
	fn generate(
		&mut self,
		_: ModelInput<'_>,
		_: &CancellationToken,
		emit: &mut dyn FnMut(&str) -> bool,
	) -> Result<(), ModelError> {
		assert!(!emit(&"🗡é".repeat(6000)));
		Ok(())
	}
}

fn setup(model: Option<Box<dyn ModelProvider>>) -> (Session, Receiver<WorkerEvent>) {
	let (out, events) = mpsc::channel();
	let session = Session::new(
		WorkerScope::new("active-project".into(), 10).unwrap(),
		NoReferences,
		model,
		move |event| {
			out.send(event).unwrap();
		},
	)
	.unwrap();
	(session, events)
}

fn request(id: &str) -> Value {
	json!({
		"type": "start",
		"request": {
			"id": id,
			"scope": { "project_id": "active-project", "workspace_generation": 10, "revision": 42 },
			"question": "How is this object animated?",
			"selection": {
				"workspace_generation": 10,
				"revision": 42,
				"content_revision": 6,
				"address": {
					"kind": "linked", "scene": "scene-2", "root": "second-root",
					"address": { "kind": "object", "placement_path": ["hinge"], "source_entity": "same-id" }
				},
				"entity_key": "instance-2",
				"name": "Repeated label",
				"transform": { "position": [1, 2, 3], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
				"parent": "second-root",
				"components": [{ "id": "animator", "type_id": "animator", "schema_version": 1, "enabled": true }],
				"selected_component": {
					"id": "animator", "schema_version": 1, "enabled": true,
					"data": { "kind": "animator", "settings": {"graph": "graphs/doors.json"} }
				},
				"capabilities": ["inspect_object", "inspect_component"]
			}
		}
	})
}

fn dispatch(session: &Session, input: Value) {
	session.handle(serde_json::from_value::<WorkerInput>(input).unwrap());
}

fn event(events: &Receiver<WorkerEvent>, id: &str) -> WorkerEventKind {
	let event = events.recv_timeout(WAIT).expect("timely Agent event");
	assert_eq!(event.id, id);
	event.kind
}

fn expect_open(events: &Receiver<WorkerEvent>, id: &str) {
	assert!(matches!(event(events, id), WorkerEventKind::Started));
	assert!(matches!(event(events, id), WorkerEventKind::Sources { .. }));
}

fn expect_status(events: &Receiver<WorkerEvent>, id: &str, expected: FinishStatus) {
	match event(events, id) {
		WorkerEventKind::Finished { status, .. } => assert_eq!(status, expected),
		other => panic!("wrong terminal event: {other:?}"),
	}
}

#[test]
fn captured_selection_and_request_id_survive_ordered_streaming() {
	let (session, events) = setup(Some(Box::new(Deterministic)));
	dispatch(&session, request("normal"));
	expect_open(&events, "normal");
	assert!(matches!(event(&events, "normal"), WorkerEventKind::Delta { text } if text == "Use "));
	assert!(matches!(
		event(&events, "normal"),
		WorkerEventKind::Delta { text } if text == "the linked Animator."
	));
	expect_status(&events, "normal", FinishStatus::Completed);
}

#[test]
fn stop_targets_the_active_request_and_discards_post_cancel_output() {
	let (session, events) = setup(Some(Box::new(WaitForStop)));
	dispatch(&session, request("active"));
	expect_open(&events, "active");
	assert!(matches!(
		event(&events, "active"),
		WorkerEventKind::Delta { .. }
	));
	dispatch(&session, json!({ "type": "stop", "id": "old" }));
	dispatch(&session, request("active"));
	dispatch(&session, request("overlap"));
	expect_status(&events, "overlap", FinishStatus::Error);
	dispatch(&session, json!({ "type": "stop", "id": "active" }));
	expect_status(&events, "active", FinishStatus::Cancelled);
	assert!(events.try_recv().is_err());
	dispatch(&session, request("next"));
	expect_open(&events, "next");
	assert!(matches!(
		event(&events, "next"),
		WorkerEventKind::Delta { .. }
	));
	dispatch(&session, json!({ "type": "stop", "id": "next" }));
	expect_status(&events, "next", FinishStatus::Cancelled);
}

#[test]
fn stale_selections_and_cross_project_requests_fail_while_keyword_only_remains_available() {
	let (session, events) = setup(None);
	let mut wrong_project = request("other");
	wrong_project["request"]["scope"]["project_id"] = json!("other-project");
	dispatch(&session, wrong_project);
	expect_status(&events, "other", FinishStatus::Error);

	let mut stale_generation = request("old-generation");
	stale_generation["request"]["scope"]["workspace_generation"] = json!(9);
	dispatch(&session, stale_generation);
	expect_status(&events, "old-generation", FinishStatus::Error);

	let mut stale = request("stale");
	stale["request"]["selection"]["revision"] = json!(41);
	dispatch(&session, stale);
	expect_status(&events, "stale", FinishStatus::Error);

	dispatch(&session, request("offline"));
	expect_open(&events, "offline");
	expect_status(&events, "offline", FinishStatus::RetrievalOnly);
}

#[test]
fn framing_and_unicode_answer_budgets_are_enforced() {
	let mut reader = BufReader::with_capacity(2, Cursor::new(b"abc\nlonger\n"));
	assert_eq!(read_frame(&mut reader, 4).unwrap().unwrap(), b"abc\n");
	assert_eq!(
		read_frame(&mut reader, 4).unwrap_err().kind(),
		ErrorKind::InvalidData
	);

	let (session, events) = setup(Some(Box::new(Oversized)));
	dispatch(&session, request("bounded"));
	expect_open(&events, "bounded");
	let mut answer = String::new();
	loop {
		match event(&events, "bounded") {
			WorkerEventKind::Delta { text } => {
				assert!(text.len() <= 2048);
				answer.push_str(&text);
			}
			WorkerEventKind::Finished { status, message } => {
				assert_eq!(status, FinishStatus::Completed);
				assert!(message.is_some());
				break;
			}
			other => panic!("unexpected event: {other:?}"),
		}
	}
	assert!(answer.len() <= 32768 && answer.len() > 32764);
	assert!("🗡é".repeat(6000).starts_with(&answer));
}
