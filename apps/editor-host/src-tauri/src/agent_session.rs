//! Hosts one scoped Agent worker and retires its child process on every exit path.

use crate::release_resources::AgentResources;
use authoring_agent::session::{
	read_frame, AgentRequest, WorkerInput, MAX_WORKER_LINE_BYTES, WORKER_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use std::{
	io::{self, BufReader, Write},
	path::Path,
	process::{Child, Command, Stdio},
	sync::{
		atomic::{AtomicBool, AtomicU64, Ordering},
		mpsc, Arc, Mutex,
	},
	thread,
	time::{Duration, Instant},
};

pub(crate) const AGENT_SESSION_EVENT: &str = "editor-agent-session";
const SCOPE_POLL: Duration = Duration::from_millis(80);
const STOP_GRACE: Duration = Duration::from_millis(700);
const WORKER_DEADLINE: Duration = Duration::from_secs(210);

#[derive(Clone)]
pub(crate) struct AgentTicket {
	id: String,
	generation: u64,
	page: u64,
	cancelled: Arc<AtomicBool>,
}

impl AgentTicket {
	fn cancel(&self) {
		self.cancelled.store(true, Ordering::Release);
	}

	fn is_cancelled(&self) -> bool {
		self.cancelled.load(Ordering::Acquire)
	}
}

pub(crate) enum ReserveError {
	Busy,
	Unavailable,
}

#[derive(Default)]
pub(crate) struct AgentSessions {
	page: AtomicU64,
	active: Mutex<Option<AgentTicket>>,
}

impl AgentSessions {
	pub(crate) fn reserve(&self, id: &str, generation: u64) -> Result<AgentTicket, ReserveError> {
		let mut active = self.active.lock().map_err(|_| ReserveError::Unavailable)?;
		if let Some(previous) = active.as_ref() {
			if previous.generation == generation {
				return Err(ReserveError::Busy);
			}
			previous.cancel();
		}
		let ticket = AgentTicket {
			id: id.to_owned(),
			generation,
			page: self.page.load(Ordering::Acquire),
			cancelled: Arc::new(AtomicBool::new(false)),
		};
		*active = Some(ticket.clone());
		Ok(ticket)
	}

	pub(crate) fn stop(&self, id: &str, generation: u64) {
		let Ok(active) = self.active.lock() else {
			return;
		};
		if let Some(ticket) = active
			.as_ref()
			.filter(|ticket| ticket.generation == generation && ticket.id == id)
		{
			ticket.cancel();
		}
	}

	pub(crate) fn invalidate(&self) {
		let Ok(mut active) = self.active.lock() else {
			return;
		};
		self.page.fetch_add(1, Ordering::AcqRel);
		if let Some(ticket) = active.take() {
			ticket.cancel();
		}
	}

	fn owns(&self, ticket: &AgentTicket) -> bool {
		self.active.lock().is_ok_and(|active| {
			active
				.as_ref()
				.is_some_and(|current| Arc::ptr_eq(&current.cancelled, &ticket.cancelled))
		})
	}

	pub(crate) fn release(&self, ticket: &AgentTicket) -> Option<bool> {
		let mut active = self.active.lock().ok()?;
		if active
			.as_ref()
			.is_some_and(|current| Arc::ptr_eq(&current.cancelled, &ticket.cancelled))
		{
			return active.take().map(|current| current.is_cancelled());
		}
		None
	}

	fn page_is_current(&self, ticket: &AgentTicket) -> bool {
		self.page.load(Ordering::Acquire) == ticket.page
	}
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}

pub(crate) fn run_agent_worker(
	sessions: &AgentSessions,
	ticket: &AgentTicket,
	resources: AgentResources,
	project_root: Option<&Path>,
	request: AgentRequest,
	is_foreground: impl Fn() -> bool,
	publish: impl Fn(Value) -> bool,
) {
	let is_current = || sessions.owns(ticket) && is_foreground();
	let deliver = |mut event: Value| {
		if !sessions.page_is_current(ticket) || !is_foreground() {
			return false;
		}
		event["workspace_generation"] = json!(ticket.generation);
		publish(event)
	};

	let result = if is_current() && !ticket.is_cancelled() {
		drive_worker(
			ticket,
			resources,
			project_root,
			request,
			&is_current,
			&deliver,
		)
	} else {
		Ok(None)
	};
	let Some(was_stopped) = sessions.release(ticket) else {
		return;
	};
	let terminal = match (was_stopped, result) {
		(true, _) => Some(terminal(ticket, "cancelled", None)),
		(false, Ok(frame)) => frame,
		(false, Err(_)) => Some(terminal(
			ticket,
			"error",
			Some("The bundled Agent worker stopped. Try the request again."),
		)),
	};
	if let Some(event) = terminal {
		deliver(event);
	}
}

fn drive_worker(
	ticket: &AgentTicket,
	resources: AgentResources,
	project_root: Option<&Path>,
	request: AgentRequest,
	is_current: &impl Fn() -> bool,
	deliver: &impl Fn(Value) -> bool,
) -> io::Result<Option<Value>> {
	let mut child = start_child(&resources, project_root, &request)?;
	let mut input = child.0.stdin.take().ok_or_else(unavailable)?;
	let output = child.0.stdout.take().ok_or_else(unavailable)?;
	let (control, commands) = mpsc::sync_channel::<WorkerInput>(1);
	thread::Builder::new()
		.name("editor-agent-commands".into())
		.spawn(move || {
			if write_input(
				&mut input,
				WorkerInput::Start {
					request: Box::new(request),
				},
			)
			.is_ok()
			{
				while let Ok(command) = commands.recv() {
					if write_input(&mut input, command).is_err() {
						break;
					}
				}
			}
		})?;

	let (send, receive) = mpsc::sync_channel::<Option<Value>>(8);
	thread::Builder::new()
		.name("editor-agent-events".into())
		.spawn(move || {
			let mut output = BufReader::new(output);
			while let Ok(Some(line)) = read_frame(&mut output, MAX_WORKER_LINE_BYTES) {
				let frame = serde_json::from_slice(&line).ok();
				let valid = frame.is_some();
				if send.send(frame).is_err() || !valid {
					break;
				}
			}
		})?;

	let deadline = Instant::now() + WORKER_DEADLINE;
	let mut stop_deadline = None;
	while is_current() {
		if ticket.is_cancelled() && stop_deadline.is_none() {
			let _ = control.try_send(WorkerInput::Stop {
				id: ticket.id.clone(),
			});
			stop_deadline = Some(Instant::now() + STOP_GRACE);
		}
		if Instant::now() >= stop_deadline.unwrap_or(deadline) {
			break;
		}
		let frame = match receive.recv_timeout(SCOPE_POLL) {
			Ok(Some(frame)) => frame,
			Err(mpsc::RecvTimeoutError::Timeout) => continue,
			_ => return Err(unavailable()),
		};
		if frame.get("id").and_then(Value::as_str) != Some(ticket.id.as_str()) {
			return Err(unavailable());
		}
		let kind = frame.get("type").and_then(Value::as_str);
		if !matches!(kind, Some("started" | "sources" | "delta" | "finished")) {
			return Err(unavailable());
		}
		if ticket.is_cancelled() {
			if kind == Some("finished") {
				break;
			}
			continue;
		}
		if kind == Some("finished") {
			return Ok(Some(frame));
		}
		if !deliver(frame) {
			return Ok(None);
		}
	}
	if Instant::now() >= deadline {
		return Err(unavailable());
	}
	Ok(None)
}

fn start_child(
	resources: &AgentResources,
	project_root: Option<&Path>,
	request: &AgentRequest,
) -> io::Result<OwnedChild> {
	let mut command = Command::new(&resources.executable);
	command
		.arg("--resources")
		.arg(&resources.root)
		.arg("--project-id")
		.arg(&request.scope.project_id)
		.arg("--workspace-generation")
		.arg(request.scope.workspace_generation.to_string())
		.arg("--protocol-version")
		.arg(WORKER_PROTOCOL_VERSION.to_string())
		.current_dir(&resources.root)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::null());
	if let Some(root) = project_root {
		command.arg("--project-root").arg(root);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		const CREATE_NO_WINDOW: u32 = 0x0800_0000;
		command.creation_flags(CREATE_NO_WINDOW);
	}
	command.spawn().map(OwnedChild)
}

fn write_input(pipe: &mut impl Write, input: WorkerInput) -> io::Result<()> {
	let mut bytes = serde_json::to_vec(&input)?;
	if bytes.len() >= MAX_WORKER_LINE_BYTES {
		return Err(io::Error::new(
			io::ErrorKind::InvalidInput,
			"Agent request frame is too large",
		));
	}
	bytes.push(b'\n');
	pipe.write_all(&bytes)?;
	pipe.flush()
}

fn terminal(ticket: &AgentTicket, status: &str, message: Option<&str>) -> Value {
	let mut event = json!({
		"id": ticket.id,
		"type": "finished",
		"status": status,
	});
	if let Some(message) = message {
		event["message"] = json!(message);
	}
	event
}

fn unavailable() -> io::Error {
	io::Error::other("The bundled Agent worker is unavailable")
}

#[cfg(test)]
#[path = "../tests/unit/tests_agent_session.rs"]
mod tests;
