//! The MCP stdio adapter relays two bounded, read-only operations into the live Editor.

use authoring_agent::{
	mcp_wire::{self as wire, DeadlineRead, Operation, Request, Response},
	session::read_frame,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
	env,
	io::{self, BufRead, BufReader, Write},
	net::TcpStream,
	time::Duration,
};

const PREFERRED_VERSION: &str = "2025-11-25";
const PREVIOUS_VERSION: &str = "2025-06-18";
const HOST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Initialize {
	protocol_version: String,
	#[serde(rename = "capabilities")]
	_capabilities: serde_json::Map<String, Value>,
	client_info: ClientInfo,
}

#[derive(Deserialize)]
struct ClientInfo {
	name: String,
	version: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct List {
	cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
	name: String,
	#[serde(default = "empty_object")]
	arguments: Value,
}

#[derive(PartialEq)]
enum Phase {
	New,
	Initialized,
	Ready,
}

struct EditorLink {
	port: u16,
	token: String,
}

impl EditorLink {
	fn from_env() -> Result<Self, &'static str> {
		let port = env::var(wire::PORT_ENV)
			.ok()
			.and_then(|value| value.parse::<u16>().ok())
			.filter(|port| *port != 0)
			.ok_or("Set the Editor MCP port from the granted connection configuration")?;
		let token = env::var(wire::TOKEN_ENV)
			.ok()
			.filter(|token| {
				token.len() == 36 && token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
			})
			.ok_or("Set the Editor MCP token from the granted connection configuration")?;
		Ok(Self { port, token })
	}

	fn send(&self, operation: Operation) -> Response {
		let result = (|| -> io::Result<_> {
			let mut bytes = serde_json::to_vec(&Request {
				token: self.token.clone(),
				operation,
			})?;
			if bytes.len() >= wire::MAX_REQUEST_BYTES {
				return Err(io::ErrorKind::InvalidData.into());
			}
			bytes.push(b'\n');
			let stream =
				TcpStream::connect_timeout(&wire::address(self.port).into(), HOST_TIMEOUT)?;
			wire::write_frame(&stream, &bytes, HOST_TIMEOUT)?;
			let mut reader = BufReader::new(DeadlineRead::new(&stream, HOST_TIMEOUT));
			let line = read_frame(&mut reader, wire::MAX_RESPONSE_BYTES)?
				.ok_or(io::ErrorKind::UnexpectedEof)?;
			Ok(serde_json::from_slice::<Response>(&line)?)
		})();
		result.unwrap_or(Response::Denied)
	}
}

struct Session {
	phase: Phase,
	editor: EditorLink,
}

impl Session {
	fn respond(&mut self, line: &[u8]) -> Option<Value> {
		let message: Value = match serde_json::from_slice(line) {
			Ok(message) => message,
			Err(_) => return Some(error(Value::Null, -32700, "Invalid JSON")),
		};
		let method = message.get("method").and_then(Value::as_str);
		let id = message.get("id");
		let valid_id = id.is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
		let valid_envelope = message["jsonrpc"] == "2.0"
			&& valid_id
			&& method.is_some()
			&& message.get("params").is_none_or(Value::is_object);
		if !valid_envelope {
			return Some(error(Value::Null, -32600, "Invalid JSON-RPC request"));
		}
		let method = method?;
		let Some(id) = id.cloned() else {
			if method == "notifications/initialized" && self.phase == Phase::Initialized {
				self.phase = Phase::Ready;
			}
			return None;
		};
		let mut params = message.get("params").cloned().unwrap_or_else(empty_object);
		// The MCP base protocol reserves optional request metadata.
		if params
			.as_object_mut()
			.and_then(|params| params.remove("_meta"))
			.is_some_and(|metadata| !metadata.is_object())
		{
			return Some(error(id, -32602, "Invalid request metadata"));
		}

		let reply = match method {
			"ping" => success(id, empty_object()),
			"initialize" if self.phase == Phase::New => self.initialize(id, params),
			"initialize" => error(id, -32600, "The connection was already initialized"),
			_ if self.phase != Phase::Ready => {
				error(id, -32002, "Initialize the MCP connection first")
			}
			"tools/list" => self.list(id, params),
			"tools/call" => self.call(id, params),
			_ => error(id, -32601, "Method not found"),
		};
		Some(reply)
	}

	fn initialize(&mut self, id: Value, params: Value) -> Value {
		let Ok(params) = serde_json::from_value::<Initialize>(params) else {
			return error(
				id,
				-32602,
				"Initialization requires version, client information and capabilities",
			);
		};
		if params.protocol_version.is_empty()
			|| params.client_info.name.is_empty()
			|| params.client_info.version.is_empty()
		{
			return error(
				id,
				-32602,
				"Initialization requires version and nonempty client information",
			);
		}
		let protocol_version = if params.protocol_version == PREVIOUS_VERSION {
			PREVIOUS_VERSION
		} else {
			PREFERRED_VERSION
		};
		self.phase = Phase::Initialized;
		success(
			id,
			json!({
				"protocolVersion": protocol_version,
				"capabilities": {"tools":{"listChanged":false}},
				"serverInfo":{"name":"game-engine-editor","version":env!("CARGO_PKG_VERSION")}
			}),
		)
	}

	fn list(&self, id: Value, params: Value) -> Value {
		let Ok(params) = serde_json::from_value::<List>(params) else {
			return error(id, -32602, "Invalid tool-list parameters");
		};
		if params.cursor.is_some() {
			return error(id, -32602, "This tool list is returned on one page");
		}
		match self.editor.send(Operation::List) {
			Response::Ok(Value::Array(mut tools)) => {
				tools.retain(|tool| tool["name"].as_str().is_some_and(read_tool));
				success(id, json!({"tools":tools}))
			}
			Response::ToolError(message) => error(id, -32000, &message),
			_ => error(
				id,
				-32001,
				"Editor access is unavailable or the grant expired",
			),
		}
	}

	fn call(&self, id: Value, params: Value) -> Value {
		let Ok(Call {
			name, arguments, ..
		}) = serde_json::from_value(params)
		else {
			return error(id, -32602, "Invalid tool-call parameters");
		};
		if !read_tool(&name) {
			return error(id, -32602, "Unknown read-only tool");
		}
		if !arguments.is_object() {
			return error(id, -32602, "Tool arguments must be an object");
		}
		match self.editor.send(Operation::Call { name, arguments }) {
			Response::Ok(payload) if payload.is_object() => success(
				id,
				json!({
					"content":[{"type":"text","text":payload.to_string()}],
					"structuredContent": payload,
					"isError":false
				}),
			),
			Response::InvalidArguments => {
				error(id, -32602, "Tool arguments do not match the schema")
			}
			Response::ToolError(message) => tool_error(id, &message),
			Response::Ok(_) => tool_error(id, "Invalid Editor tool response"),
			Response::Denied => tool_error(id, "Editor access is unavailable or the grant expired"),
		}
	}
}

pub(super) fn run() -> Result<(), &'static str> {
	let mut session = Session {
		phase: Phase::New,
		editor: EditorLink::from_env()?,
	};
	serve(
		&mut io::stdin().lock(),
		&mut io::stdout().lock(),
		&mut session,
	)
	.map_err(|_| "The MCP stdio connection closed")
}

fn serve(
	reader: &mut impl BufRead,
	writer: &mut impl Write,
	session: &mut Session,
) -> io::Result<()> {
	loop {
		match read_frame(reader, wire::MAX_REQUEST_BYTES) {
			Ok(Some(line)) => {
				if let Some(reply) = session.respond(&line) {
					write_reply(writer, &reply)?;
				}
			}
			Ok(None) => return Ok(()),
			Err(error_) if error_.kind() == io::ErrorKind::InvalidData => {
				return write_reply(
					writer,
					&error(Value::Null, -32700, "MCP request exceeds the frame limit"),
				);
			}
			Err(error) => return Err(error),
		}
	}
}

fn write_reply(writer: &mut impl Write, reply: &Value) -> io::Result<()> {
	serde_json::to_writer(&mut *writer, reply)?;
	writer.write_all(b"\n")?;
	writer.flush()
}

fn read_tool(name: &str) -> bool {
	matches!(name, "editor.inspect" | "assets.query")
}

fn empty_object() -> Value {
	json!({})
}

fn success(id: Value, result: Value) -> Value {
	json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error(id: Value, code: i32, message: &str) -> Value {
	json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn tool_error(id: Value, message: &str) -> Value {
	success(
		id,
		json!({"content":[{"type":"text","text":message}],"isError":true}),
	)
}
