//! One expiring capability for external read-only access to the live Editor.

use crate::{editor::EditorWorkspace, state::EditorExitState};
use authoring_agent::{
	mcp_wire::{self as wire, DeadlineRead, Operation, Request, Response},
	session::read_frame,
	Bundle, ToolDescriptor, ToolError,
};
use runtime::asset_catalog::AssetId;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
	io::{self, BufReader},
	net::{TcpListener, TcpStream},
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

mod live_tools;
#[cfg(test)]
mod tests;

use live_tools::{LiveTools, WorkspaceSlot};

pub(crate) const GRANT_LIFETIME: Duration = Duration::from_secs(600);
const IO_DEADLINE: Duration = Duration::from_millis(500);
const MAX_READS_PER_SECOND: u8 = 24;

#[derive(Default)]
pub(crate) struct Access {
	state: Mutex<AccessState>,
}

#[derive(Default)]
struct AccessState {
	page: u64,
	active: Option<Arc<Lease>>,
}

struct Lease {
	generation: u64,
	digest: [u8; 32],
	expires: Instant,
}

impl Lease {
	fn matches(&self, token: &str) -> bool {
		if token.len() != 36 {
			return false;
		}
		let digest = Sha256::digest(token.as_bytes());
		self.digest
			.iter()
			.zip(digest)
			.fold(0, |difference, (expected, actual)| {
				difference | (expected ^ actual)
			}) == 0
	}
}

#[derive(Serialize)]
pub struct Connection {
	pub workspace_generation: u64,
	pub port: u16,
	pub token: String,
	pub expires_in_seconds: u64,
	pub worker_path: Option<String>,
}

impl Access {
	pub(crate) fn page(&self) -> Result<u64, String> {
		self.state
			.lock()
			.map(|state| state.page)
			.map_err(|_| unavailable())
	}

	pub(crate) fn invalidate(&self) {
		if let Ok(mut state) = self.state.lock() {
			state.page = state.page.wrapping_add(1);
			state.active = None;
		}
	}

	pub(crate) fn issue(
		self: &Arc<Self>,
		captured_page: u64,
		workspace: &EditorWorkspace,
		shared: WorkspaceSlot,
		exit: Arc<EditorExitState>,
		worker_path: Option<String>,
	) -> Result<Connection, String> {
		self.issue_for(
			captured_page,
			workspace.generation,
			shared,
			exit,
			worker_path,
			GRANT_LIFETIME,
		)
	}

	fn issue_for(
		self: &Arc<Self>,
		captured_page: u64,
		generation: u64,
		shared: WorkspaceSlot,
		exit: Arc<EditorExitState>,
		worker_path: Option<String>,
		lifetime: Duration,
	) -> Result<Connection, String> {
		if exit.is_committed() {
			return Err(unavailable());
		}
		let listener = TcpListener::bind(wire::address(0)).map_err(|_| unavailable())?;
		listener.set_nonblocking(true).map_err(|_| unavailable())?;
		let port = listener.local_addr().map_err(|_| unavailable())?.port();
		let token = AssetId::generate().as_str().to_owned();
		let lease = Arc::new(Lease {
			generation,
			digest: Sha256::digest(token.as_bytes()).into(),
			expires: Instant::now() + lifetime,
		});
		let bundle = Bundle::builtin().map_err(|_| unavailable())?;
		let mut state = self.state.lock().map_err(|_| unavailable())?;
		if state.page != captured_page {
			return Err("The Editor page changed. Request access again.".into());
		}
		let server = Server {
			access: Arc::clone(self),
			lease: Arc::clone(&lease),
			shared,
			exit,
			bundle,
		};
		thread::Builder::new()
			.name("editor-mcp-grant".into())
			.spawn(move || server.serve(listener))
			.map_err(|_| unavailable())?;
		state.active = Some(lease);
		Ok(Connection {
			workspace_generation: generation,
			port,
			token,
			expires_in_seconds: lifetime.as_secs(),
			worker_path,
		})
	}

	pub(crate) fn revoke(&self, generation: u64, token: &str) -> bool {
		let Ok(mut state) = self.state.lock() else {
			return false;
		};
		if !state
			.active
			.as_ref()
			.is_some_and(|active| active.generation == generation && active.matches(token))
		{
			return false;
		}
		state.active = None;
		true
	}

	fn authorizes(&self, lease: &Arc<Lease>) -> bool {
		Instant::now() < lease.expires
			&& self.state.lock().is_ok_and(|state| {
				state
					.active
					.as_ref()
					.is_some_and(|current| Arc::ptr_eq(current, lease))
			})
	}

	fn retire(&self, lease: &Arc<Lease>) {
		if let Ok(mut state) = self.state.lock() {
			if state
				.active
				.as_ref()
				.is_some_and(|current| Arc::ptr_eq(current, lease))
			{
				state.active = None;
			}
		}
	}
}

struct ReadBudget {
	period_started: Instant,
	used: u8,
}

impl ReadBudget {
	fn allow(&mut self, now: Instant) -> bool {
		if now.saturating_duration_since(self.period_started) >= Duration::from_secs(1) {
			self.period_started = now;
			self.used = 0;
		}
		if self.used == MAX_READS_PER_SECOND {
			return false;
		}
		self.used += 1;
		true
	}
}

struct Server {
	access: Arc<Access>,
	lease: Arc<Lease>,
	shared: WorkspaceSlot,
	exit: Arc<EditorExitState>,
	bundle: Bundle,
}

impl Server {
	fn serve(&self, listener: TcpListener) {
		let mut budget = ReadBudget {
			period_started: Instant::now(),
			used: 0,
		};
		while self.current() {
			match listener.accept() {
				Ok((stream, _)) => {
					let _ = self.handle(&stream, &mut budget);
				}
				Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
					thread::sleep(Duration::from_millis(40));
				}
				Err(_) => break,
			}
		}
		self.access.retire(&self.lease);
	}

	fn grant_active(&self) -> bool {
		!self.exit.is_committed() && self.access.authorizes(&self.lease)
	}

	fn current(&self) -> bool {
		self.grant_active()
			&& self.shared.lock().is_ok_and(|slot| {
				slot.as_ref()
					.is_some_and(|workspace| workspace.generation == self.lease.generation)
			})
	}

	fn handle(&self, stream: &TcpStream, budget: &mut ReadBudget) -> io::Result<()> {
		let Some(bytes) = read_frame(
			&mut BufReader::new(DeadlineRead::new(stream, IO_DEADLINE)),
			wire::MAX_REQUEST_BYTES,
		)?
		else {
			return Ok(());
		};
		if !bytes.ends_with(b"\n") {
			return Ok(());
		}
		let Ok(request) = serde_json::from_slice::<Request>(&bytes) else {
			return Ok(());
		};
		let response = if !self.lease.matches(&request.token) || !self.current() {
			Response::Denied
		} else if !budget.allow(Instant::now()) {
			Response::ToolError("Editor read limit reached. Try again in a moment.".into())
		} else {
			self.dispatch(request.operation)
		};

		let response = if self.current() {
			response
		} else {
			Response::Denied
		};
		let mut bytes = serde_json::to_vec(&response)?;
		if bytes.len() >= wire::MAX_RESPONSE_BYTES {
			return Err(io::ErrorKind::InvalidData.into());
		}
		bytes.push(b'\n');
		wire::write_frame(stream, &bytes, IO_DEADLINE)
	}

	fn dispatch(&self, operation: Operation) -> Response {
		let authorizes = || self.grant_active();
		let service = LiveTools::new(&self.shared, self.lease.generation, &authorizes);
		let Some(scope) = service.scope() else {
			return Response::Denied;
		};
		let response = match operation {
			Operation::List => Response::Ok(Value::Array(
				self.bundle
					.available_tools(&service, &scope)
					.into_iter()
					.map(tool_descriptor)
					.collect(),
			)),
			Operation::Call { name, arguments } => {
				if !matches!(name.as_str(), "editor.inspect" | "assets.query") {
					return Response::ToolError("Unknown read-only tool".into());
				}
				match self
					.bundle
					.invoke(&service, &scope, &name, arguments.to_string().as_bytes())
				{
					Ok(value) => Response::Ok(value),
					Err(ToolError::InvalidArguments) => Response::InvalidArguments,
					Err(error) => Response::ToolError(error.to_string()),
				}
			}
		};
		if service.scope().as_ref() == Some(&scope) {
			response
		} else {
			Response::ToolError("The authored context changed. Read it again.".into())
		}
	}
}

fn tool_descriptor(mut tool: ToolDescriptor) -> Value {
	tool.input_schema.meta_schema = None;
	json!({
		"name": tool.name,
		"description": tool.description,
		"inputSchema": tool.input_schema,
		"annotations": {"readOnlyHint": true, "openWorldHint": false}
	})
}

fn unavailable() -> String {
	"The local MCP connection is unavailable".into()
}
