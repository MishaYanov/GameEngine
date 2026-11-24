//! Private loopback contract between the MCP stdio process and the Editor.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
	io::{self, Read, Write},
	net::{Ipv4Addr, SocketAddrV4, TcpStream},
	time::{Duration, Instant},
};

/// Bound a call before deserializing its selection or arguments.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// Room for a bounded Agent tool result and the private envelope.
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024;
pub const PORT_ENV: &str = "GAME_ENGINE_MCP_PORT";
pub const TOKEN_ENV: &str = "GAME_ENGINE_MCP_TOKEN";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
	pub token: String,
	pub operation: Operation,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
	List,
	Call { name: String, arguments: Value },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum Response {
	Ok(Value),
	Denied,
	InvalidArguments,
	ToolError(String),
}

pub fn address(port: u16) -> SocketAddrV4 {
	SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)
}

/// Every frame has an overall read deadline, including when it arrives byte by byte.
pub struct DeadlineRead<'a> {
	stream: &'a TcpStream,
	deadline: Instant,
}

impl<'a> DeadlineRead<'a> {
	pub fn new(stream: &'a TcpStream, timeout: Duration) -> Self {
		Self {
			stream,
			deadline: Instant::now() + timeout,
		}
	}
}

impl Read for DeadlineRead<'_> {
	fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
		let remaining = self.deadline.saturating_duration_since(Instant::now());
		if remaining.is_zero() {
			return Err(io::ErrorKind::TimedOut.into());
		}
		self.stream.set_read_timeout(Some(remaining))?;
		self.stream.read(buffer)
	}
}

/// Finish sending a complete frame within one deadline.
pub fn write_frame(mut stream: &TcpStream, bytes: &[u8], timeout: Duration) -> io::Result<()> {
	let deadline = Instant::now() + timeout;
	let mut pending = bytes;
	while !pending.is_empty() {
		let remaining = deadline.saturating_duration_since(Instant::now());
		if remaining.is_zero() {
			return Err(io::ErrorKind::TimedOut.into());
		}
		stream.set_write_timeout(Some(remaining))?;
		match stream.write(pending) {
			Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
			Ok(count) => pending = &pending[count..],
			Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
			Err(error) => return Err(error),
		}
	}
	Ok(())
}
