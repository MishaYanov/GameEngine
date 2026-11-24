use super::*;
use crate::{project_location::ProjectLocation, test_project::ProjectFixture};
use bridge::editor::{EditorCommandDto as Command, EditorCommandRequest};
use std::{fs, io::Write, net::TcpStream, path::Path, process::Stdio};

#[test]
fn live_workspace_grants_allow_bounded_inspection_and_reject_old_tokens() {
	let first = ProjectFixture::new("component-lab");
	let second = ProjectFixture::new("camera-room");
	let shared = Arc::new(Mutex::new(Some(open(&first))));
	let access = Arc::new(Access::default());
	let exit = Arc::new(EditorExitState::default());
	let first_grant = issue(&access, &shared, &exit);

	let disk_before = fs::read(first.manifest_path()).unwrap();
	let revision_before = shared
		.lock()
		.unwrap()
		.as_ref()
		.unwrap()
		.backend
		.session()
		.revision();
	let Response::Ok(discovered) = call(&first_grant, Operation::List).unwrap() else {
		panic!("authorized discovery failed");
	};
	assert_eq!(discovered.as_array().unwrap().len(), 2);
	assert_eq!(discovered[0]["annotations"]["readOnlyHint"], true);
	assert_eq!(discovered[1]["name"], "assets.query");
	assert!(matches!(
		relay(
			first_grant.port,
			"597de839-42fd-4f39-8c8b-7bb8a8752739",
			Operation::List
		),
		Some(Response::Denied)
	));
	assert!(matches!(
		call(
			&first_grant,
			Operation::Call {
				name: "scene.apply_batch".into(),
				arguments: json!({})
			}
		),
		Some(Response::ToolError(_))
	));
	assert!(matches!(
		call(
			&first_grant,
			Operation::Call {
				name: "assets.query".into(),
				arguments: json!({"query":"robot","limit":40})
			}
		),
		Some(Response::InvalidArguments)
	));
	let Response::Ok(assets) = call(
		&first_grant,
		Operation::Call {
			name: "assets.query".into(),
			arguments: json!({"query":"two-joint","limit":1}),
		},
	)
	.unwrap() else {
		panic!("authorized assets query failed");
	};
	assert_eq!(assets["assets"].as_array().unwrap().len(), 1);
	assert!(assets["assets"][0]["path"]
		.as_str()
		.unwrap()
		.starts_with("characters/"));
	assert_eq!(fs::read(first.manifest_path()).unwrap(), disk_before);
	assert_eq!(
		shared
			.lock()
			.unwrap()
			.as_ref()
			.unwrap()
			.backend
			.session()
			.revision(),
		revision_before
	);

	{
		let mut slot = shared.lock().unwrap();
		let workspace = slot.as_mut().unwrap();
		workspace
			.backend
			.execute(EditorCommandRequest {
				expected_revision: revision_before,
				command: Command::Select {
					keys: vec!["lab-player".into()],
				},
			})
			.unwrap();
	}
	let Response::Ok(inspection) = call(
		&first_grant,
		Operation::Call {
			name: "editor.inspect".into(),
			arguments: json!({}),
		},
	)
	.unwrap() else {
		panic!("live inspection failed");
	};
	assert_eq!(inspection["scene"], "component-lab");
	assert_eq!(inspection["selection"]["entity_key"], "lab-player");
	assert!(inspection["revision"].as_u64().unwrap() > revision_before);

	if let Some(worker) = std::env::var_os("GAME_ENGINE_TEST_MCP_WORKER") {
		let replies = external_client(Path::new(&worker), &first_grant);
		assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 2);
		assert_eq!(
			replies[2]["result"]["structuredContent"]["selection"]["entity_key"],
			"lab-player"
		);
	}

	{
		let mut slot = shared.lock().unwrap();
		let workspace = slot.as_mut().unwrap();
		let separator = std::path::MAIN_SEPARATOR;
		let mut alias = workspace.asset_root.as_os_str().to_owned();
		alias.push(format!("{separator}..{separator}assets"));
		workspace.asset_root = alias.into();
	}
	assert!(matches!(call(&first_grant, Operation::Call {
		name: "assets.query".into(), arguments: json!({"query":"two-joint"})
	}), Some(Response::ToolError(message)) if message.contains("asset root changed")));

	*shared.lock().unwrap() = Some(open(&second));
	assert_denied(&first_grant);
	if let Some(worker) = std::env::var_os("GAME_ENGINE_TEST_MCP_WORKER") {
		let replies = external_client(Path::new(&worker), &first_grant);
		assert_eq!(replies[1]["error"]["code"], -32001);
		assert_eq!(replies[2]["result"]["isError"], true);
	}

	let second_grant = issue(&access, &shared, &exit);
	let Response::Ok(inspection) = call(
		&second_grant,
		Operation::Call {
			name: "editor.inspect".into(),
			arguments: json!({}),
		},
	)
	.unwrap() else {
		panic!("new workspace grant failed");
	};
	assert_eq!(inspection["scene"], "camera-room");
	assert!(!access.revoke(first_grant.workspace_generation, &first_grant.token));
	assert!(access.revoke(second_grant.workspace_generation, &second_grant.token));
	assert_denied(&second_grant);
}

#[test]
fn expiry_and_webview_reload_retire_the_grant() {
	let fixture = ProjectFixture::new("component-lab");
	let shared = Arc::new(Mutex::new(Some(open(&fixture))));
	let access = Arc::new(Access::default());
	let exit = Arc::new(EditorExitState::default());
	let generation = shared.lock().unwrap().as_ref().unwrap().generation;
	let short = access
		.issue_for(
			access.page().unwrap(),
			generation,
			Arc::clone(&shared),
			Arc::clone(&exit),
			None,
			Duration::from_millis(150),
		)
		.unwrap();
	thread::sleep(Duration::from_millis(180));
	assert_denied(&short);
	let page = access.page().unwrap();
	let grant = issue(&access, &shared, &exit);
	access.invalidate();
	assert_denied(&grant);
	assert!(access
		.issue_for(page, generation, shared, exit, None, GRANT_LIFETIME)
		.is_err());
}

#[test]
fn shared_budget_caps_authenticated_reads_and_resets() {
	let now = Instant::now();
	let mut budget = ReadBudget {
		period_started: now,
		used: 0,
	};
	for _ in 0..MAX_READS_PER_SECOND {
		assert!(budget.allow(now));
	}
	assert!(!budget.allow(now));
	assert!(budget.allow(now + Duration::from_secs(1)));
}

fn open(fixture: &ProjectFixture) -> EditorWorkspace {
	let location = ProjectLocation::from_manifest(&fixture.manifest_path()).unwrap();
	EditorWorkspace::open_project_with_notifier(location, || {}).unwrap()
}

fn issue(access: &Arc<Access>, shared: &WorkspaceSlot, exit: &Arc<EditorExitState>) -> Connection {
	let workspace = shared.lock().unwrap();
	access
		.issue(
			access.page().unwrap(),
			workspace.as_ref().unwrap(),
			Arc::clone(shared),
			Arc::clone(exit),
			None,
		)
		.unwrap()
}

fn call(grant: &Connection, operation: Operation) -> Option<Response> {
	relay(grant.port, &grant.token, operation)
}

fn relay(port: u16, token: &str, operation: Operation) -> Option<Response> {
	let mut stream =
		TcpStream::connect_timeout(&wire::address(port).into(), Duration::from_secs(1)).ok()?;
	let request = serde_json::to_string(&Request {
		token: token.into(),
		operation,
	})
	.unwrap();
	writeln!(stream, "{request}").ok()?;
	let bytes = read_frame(
		&mut BufReader::new(DeadlineRead::new(&stream, Duration::from_secs(2))),
		wire::MAX_RESPONSE_BYTES,
	)
	.ok()??;
	serde_json::from_slice(&bytes).ok()
}

fn assert_denied(grant: &Connection) {
	assert!(matches!(
		call(grant, Operation::List),
		None | Some(Response::Denied)
	));
}

fn external_client(worker: &Path, grant: &Connection) -> Vec<Value> {
	let mut child = std::process::Command::new(worker)
		.arg("--mcp")
		.env(wire::PORT_ENV, grant.port.to_string())
		.env(wire::TOKEN_ENV, &grant.token)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.unwrap();
	let mut stdin = child.stdin.take().unwrap();
	for request in [
		json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
			"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}
		}}),
		json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
		json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
		json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"editor.inspect"}}),
	] {
		writeln!(stdin, "{request}").unwrap();
	}
	drop(stdin);
	let output = child.wait_with_output().unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	String::from_utf8(output.stdout)
		.unwrap()
		.lines()
		.map(|line| serde_json::from_str(line).unwrap())
		.collect()
}
