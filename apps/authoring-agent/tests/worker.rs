use serde_json::{Value, json};
use std::{
	env, fs,
	io::{BufRead, BufReader, Read, Write},
	net::{Ipv4Addr, TcpListener, TcpStream},
	path::PathBuf,
	process::{Child, ChildStdin, Command, Stdio},
	sync::{
		atomic::{AtomicUsize, Ordering},
		mpsc::{self, Receiver},
	},
	thread,
	time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);

struct Fixture(PathBuf);

impl Fixture {
	fn new() -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.canonicalize()
			.unwrap()
			.join(format!(
				"qa-agent-worker-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		let help = root.join("agent/engine-help");
		fs::create_dir_all(&help).unwrap();
		fs::write(
			root.join("agent/bundle.json"),
			include_bytes!(
				"../../../crates/authoring-agent/profiles/game-development-support.json"
			),
		)
		.unwrap();
		fs::write(
			help.join("editor-workspace.md"),
			"# Capsule marker\nUse the capsule marker in a preview.\n",
		)
		.unwrap();
		fs::create_dir(root.join("project")).unwrap();
		fs::create_dir(root.join("launch")).unwrap();
		let fixture = Self(root);
		fixture.save_project("initial capsule marker project");
		fixture
	}

	fn save_project(&self, name: &str) {
		fs::write(
			self.0.join("project/project.json"),
			json!({ "title": name }).to_string(),
		)
		.unwrap();
	}
}

impl Drop for Fixture {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

struct Worker {
	child: Child,
	input: ChildStdin,
	events: Receiver<Value>,
}

impl Worker {
	fn spawn(fixture: &Fixture, port: Option<u16>) -> Self {
		let mut command = Command::new(env!("CARGO_BIN_EXE_authoring-agent-worker"));
		command
			.current_dir(fixture.0.join("launch"))
			.args([
				"--project-id",
				"worker-test",
				"--workspace-generation",
				"91",
				"--protocol-version",
				"1",
			])
			.arg("--resources")
			.arg(fixture.0.join("agent"))
			.arg("--project-root")
			.arg(fixture.0.join("project"))
			.env_remove("ENGINE_AGENT_OLLAMA_MODEL")
			.env_remove("ENGINE_AGENT_OLLAMA_PORT")
			.stdin(Stdio::piped())
			.stdout(Stdio::piped());
		if let Some(port) = port {
			command
				.env("ENGINE_AGENT_OLLAMA_MODEL", "local-model:test")
				.env("ENGINE_AGENT_OLLAMA_PORT", port.to_string());
		}
		let mut child = command.spawn().unwrap();
		let input = child.stdin.take().unwrap();
		let stdout = child.stdout.take().unwrap();
		let (out, events) = mpsc::channel();
		thread::spawn(move || {
			for line in BufReader::new(stdout).lines() {
				let event = serde_json::from_str(&line.unwrap()).expect("JSON-only worker stdout");
				if out.send(event).is_err() {
					break;
				}
			}
		});
		Self {
			child,
			input,
			events,
		}
	}

	fn start(&mut self, id: &str) {
		self.send(json!({
			"type": "start",
			"request": {
				"id": id,
				"scope": {"project_id":"worker-test","workspace_generation":91,"revision":5},
				"question": "capsule marker"
			}
		}));
	}

	fn send(&mut self, request: Value) {
		writeln!(self.input, "{request}").unwrap();
	}

	fn event(&self, id: &str, kind: &str) -> Value {
		let event = self.events.recv_timeout(WAIT).expect("timely Agent event");
		assert_eq!(event["id"], id, "{event}");
		assert_eq!(event["type"], kind, "{event}");
		event
	}
}

impl Drop for Worker {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[test]
fn installed_layout_returns_cited_keyword_results_and_refreshes_project_metadata() {
	let fixture = Fixture::new();
	let mut worker = Worker::spawn(&fixture, None);
	for (id, name) in [
		("offline", "initial capsule marker project"),
		("refreshed", "updated capsule marker project"),
	] {
		fixture.save_project(name);
		worker.start(id);
		worker.event(id, "started");
		let event = worker.event(id, "sources");
		let citations = event["citations"].as_array().unwrap();
		assert!(citations.iter().any(|hit| {
			hit["citation"]["source_id"] == "editor-workspace"
				&& hit["citation"]["corpus"] == "engine-help"
				&& hit["citation"]["project_id"].is_null()
		}));
		assert!(citations.iter().any(|hit| {
			hit["citation"]["source_id"] == "project"
				&& hit["citation"]["project_id"] == "worker-test"
				&& hit["excerpt"].as_str().unwrap().contains(name)
		}));
		assert_eq!(worker.event(id, "finished")["status"], "retrieval_only");
	}
}

#[test]
fn installed_help_cites_the_current_object_authoring_guide() {
	let fixture = Fixture::new();
	let guide = fixture.0.join("agent/engine-help/authoring-objects.md");
	fs::write(guide, include_str!("../../../docs/authoring-objects.md")).unwrap();
	let mut worker = Worker::spawn(&fixture, None);
	worker.send(json!({
		"type": "start",
		"request": {
			"id": "authoring-guide",
			"scope": {"project_id":"worker-test","workspace_generation":91,"revision":5},
			"question": "Rotation Routine JSON"
		}
	}));
	worker.event("authoring-guide", "started");
	let event = worker.event("authoring-guide", "sources");
	assert!(event["citations"].as_array().unwrap().iter().any(|hit| {
		hit["citation"]["source_id"] == "object-components"
			&& hit["citation"]["corpus"] == "engine-help"
	}));
	assert_eq!(
		worker.event("authoring-guide", "finished")["status"],
		"retrieval_only"
	);
}

#[test]
fn loopback_model_stream_can_be_stopped_while_the_provider_is_waiting() {
	let fixture = Fixture::new();
	let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
	let mut worker = Worker::spawn(&fixture, Some(server.local_addr().unwrap().port()));
	let responder = thread::spawn(move || {
		let mut show = accept(&server);
		let (request, body) = read_request(&mut show);
		assert!(request.starts_with("POST /api/show HTTP/1.0"));
		assert_eq!(body, json!({"model":"local-model:test"}));
		write!(show, "HTTP/1.0 200 OK\r\n\r\n{{\"model_info\":{{}}}}").unwrap();
		drop(show);

		let mut stream = accept(&server);
		let (request, body) = read_request(&mut stream);
		assert!(request.starts_with("POST /api/generate HTTP/1.0"));
		assert_eq!(body["stream"], true);
		assert!(body["tools"].is_null());
		assert!(body["prompt"].as_str().unwrap().contains("capsule marker"));
		write!(
			stream,
			"HTTP/1.0 200 OK\r\nContent-Type: application/x-ndjson\r\n\r\n{{\"response\":\"model delta\",\"done\":false}}\n"
		).unwrap();
		stream.set_read_timeout(Some(WAIT)).unwrap();
		assert_eq!(
			stream.read(&mut [0]).unwrap(),
			0,
			"Stop closes the provider stream"
		);
	});

	worker.start("model");
	worker.event("model", "started");
	worker.event("model", "sources");
	assert_eq!(worker.event("model", "delta")["text"], "model delta");
	worker.send(json!({"type":"stop","id":"model"}));
	assert_eq!(worker.event("model", "finished")["status"], "cancelled");
	responder.join().unwrap();
}

#[test]
fn remote_model_alias_is_rejected_before_any_project_context_is_sent() {
	let fixture = Fixture::new();
	let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
	let mut worker = Worker::spawn(&fixture, Some(server.local_addr().unwrap().port()));
	let (answered, wait_for_answer) = mpsc::channel();
	let responder = thread::spawn(move || {
		let mut show = accept(&server);
		let (request, body) = read_request(&mut show);
		assert!(request.starts_with("POST /api/show HTTP/1.0"));
		assert_eq!(body, json!({"model":"local-model:test"}));
		write!(
			show,
			"HTTP/1.0 200 OK\r\n\r\n{{\"model_info\":{{}},\"remote_host\":\"https://example.invalid\"}}"
		)
		.unwrap();
		drop(show);
		wait_for_answer.recv_timeout(WAIT).unwrap();
		server.set_nonblocking(true).unwrap();
		assert_eq!(
			server.accept().unwrap_err().kind(),
			std::io::ErrorKind::WouldBlock
		);
	});

	worker.start("remote");
	worker.event("remote", "started");
	worker.event("remote", "sources");
	assert_eq!(
		worker.event("remote", "finished")["status"],
		"retrieval_only"
	);
	answered.send(()).unwrap();
	responder.join().unwrap();
}

fn accept(server: &TcpListener) -> TcpStream {
	server.set_nonblocking(true).unwrap();
	let deadline = Instant::now() + WAIT;
	loop {
		match server.accept() {
			Ok((stream, _)) => {
				stream.set_nonblocking(false).unwrap();
				stream.set_read_timeout(Some(WAIT)).unwrap();
				return stream;
			}
			Err(error)
				if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
			{
				thread::sleep(Duration::from_millis(5))
			}
			Err(error) => panic!("model connection unavailable: {error}"),
		}
	}
}

fn read_request(stream: &mut TcpStream) -> (String, Value) {
	let mut reader = BufReader::new(stream);
	let mut request = String::new();
	reader.read_line(&mut request).unwrap();
	let mut content_length = None;
	loop {
		let mut line = String::new();
		assert_ne!(reader.read_line(&mut line).unwrap(), 0);
		if line == "\r\n" {
			break;
		}
		if let Some(length) = line.strip_prefix("Content-Length: ") {
			content_length = Some(length.trim().parse::<usize>().unwrap());
		}
	}
	let mut body = vec![0; content_length.unwrap()];
	reader.read_exact(&mut body).unwrap();
	(request, serde_json::from_slice(&body).unwrap())
}
