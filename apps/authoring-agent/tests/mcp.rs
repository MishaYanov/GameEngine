use authoring_agent::mcp_wire::{self as wire, Operation, Request, Response};
use serde_json::{Value, json};
use std::{
	io::{BufRead, BufReader, Write},
	net::TcpListener,
	process::{Command, Stdio},
	thread,
	time::{Duration, Instant},
};

const TOKEN: &str = "597de839-42fd-4f39-8c8b-7bb8a8752739";
const TEST_DEADLINE: Duration = Duration::from_secs(7);

#[test]
fn independent_process_negotiates_lists_and_calls_only_read_tools() {
	let listener = TcpListener::bind(wire::address(0)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let port = listener.local_addr().unwrap().port();
	let editor = thread::spawn(move || {
		let deadline = Instant::now() + TEST_DEADLINE;
		for expected in ["list", "call"] {
			let mut stream = loop {
				match listener.accept() {
					Ok((stream, _)) => break stream,
					Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
						assert!(
							Instant::now() < deadline,
							"an MCP request never reached the host"
						);
						thread::sleep(Duration::from_millis(10));
					}
					Err(error) => panic!("cannot accept host fixture request: {error}"),
				}
			};
			stream.set_read_timeout(Some(TEST_DEADLINE)).unwrap();
			let mut line = String::new();
			BufReader::new(&stream).read_line(&mut line).unwrap();
			let request = serde_json::from_str::<Request>(&line).unwrap();
			assert_eq!(request.token, TOKEN);
			let payload = match request.operation {
				Operation::List => {
					assert_eq!(expected, "list");
					json!([{"name":"editor.inspect","inputSchema":{"type":"object"}}])
				}
				Operation::Call { name, arguments } => {
					assert_eq!(expected, "call");
					assert_eq!(name, "editor.inspect");
					assert_eq!(arguments, json!({}));
					json!({"scene":"live-scene","revision":9})
				}
			};
			writeln!(
				stream,
				"{}",
				serde_json::to_string(&Response::Ok(payload)).unwrap()
			)
			.unwrap();
		}
	});

	let input = format!(
		"{{malformed\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
		request(1, "tools/list", json!({})),
		request(
			2,
			"initialize",
			json!({
				"protocolVersion":"unsupported-version","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}
			})
		),
		json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
		request(
			3,
			"tools/list",
			json!({"_meta":{"progressToken":"fixture"}})
		),
		request(4, "tools/call", json!({"name":"editor.inspect"})),
		request(
			5,
			"tools/call",
			json!({"name":"scene.apply_batch","arguments":{}})
		),
		request(6, "filesystem/list", json!({}))
	);
	let replies = run(port, input.as_bytes());
	editor.join().unwrap();
	assert_eq!(replies.len(), 7);
	assert_eq!(replies[0]["error"]["code"], -32700);
	assert_eq!(replies[1]["error"]["code"], -32002);
	assert_eq!(replies[2]["result"]["protocolVersion"], "2025-11-25");
	assert_eq!(
		replies[2]["result"]["capabilities"],
		json!({"tools":{"listChanged":false}})
	);
	assert_eq!(replies[3]["result"]["tools"][0]["name"], "editor.inspect");
	assert_eq!(
		replies[4]["result"]["structuredContent"]["scene"],
		"live-scene"
	);
	assert_eq!(replies[4]["result"]["content"][0]["type"], "text");
	assert_eq!(replies[5]["error"]["code"], -32602);
	assert_eq!(replies[6]["error"]["code"], -32601);
}

#[test]
fn prior_protocol_and_oversized_input_have_bounded_responses() {
	let init = request(
		1,
		"initialize",
		json!({
			"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}
		}),
	);
	let mut input = format!("{init}\n").into_bytes();
	input.extend(std::iter::repeat_n(b'a', wire::MAX_REQUEST_BYTES + 1));
	let replies = run(1, &input);
	assert_eq!(replies.len(), 2);
	assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
	assert_eq!(replies[1]["error"]["code"], -32700);
	assert!(
		replies[1]["error"]["message"]
			.as_str()
			.unwrap()
			.contains("frame limit")
	);
}

fn request(id: u16, method: &str, params: Value) -> Value {
	json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn run(port: u16, input: &[u8]) -> Vec<Value> {
	let mut child = Command::new(env!("CARGO_BIN_EXE_authoring-agent-worker"))
		.arg("--mcp")
		.env(wire::PORT_ENV, port.to_string())
		.env(wire::TOKEN_ENV, TOKEN)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.unwrap();
	let write_result = child.stdin.take().unwrap().write_all(input);
	let output = child.wait_with_output().unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	if let Err(error) = write_result {
		// The worker can close stdin when it detects an oversized frame.
		assert!(matches!(
			error.kind(),
			std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
		));
	}
	BufReader::new(output.stdout.as_slice())
		.lines()
		.map(|line| serde_json::from_str(&line.unwrap()).expect("valid MCP JSON on stdout"))
		.collect()
}
