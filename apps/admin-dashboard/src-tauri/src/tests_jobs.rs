use super::*;
#[test]
fn queue_validation_cancellation_and_restart() {
	let data = crate::tests::test_directory("jobs");
	std::fs::create_dir_all(&data).unwrap();
	let mut jobs = Jobs::new(&data);
	let mut processes = Processes::new(&data);
	assert!(
		jobs.queue(&data, Kind::Build, "unknown".into(), "dev".into())
			.is_err()
	);
	let first = jobs
		.queue(&data, Kind::Build, "compiler".into(), "dev".into())
		.unwrap();
	jobs.cancel(&first.id, &mut processes).unwrap();
	assert_eq!(jobs.runs[0].state, Status::Cancelled);
	jobs.queue(&data, Kind::Test, "compiler".into(), "dev".into())
		.unwrap();
	let restored = Jobs::new(&data);
	assert_eq!(restored.runs[0].state, Status::Interrupted);
	assert_eq!(restored.runs[1].state, Status::Cancelled);
}
#[test]
fn cargo_fixture_build_and_test_outcomes() {
	let data = crate::tests::test_directory("cargo");
	std::fs::create_dir_all(data.join("src")).unwrap();
	std::fs::write(
		data.join("Cargo.toml"),
		"[package]\nname='compiler'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
	)
	.unwrap();
	std::fs::write(
		data.join("src/lib.rs"),
		"#[test] fn passing(){assert_eq!(2+2,4);} #[test] fn failing(){assert_eq!(2+2,5);}",
	)
	.unwrap();
	let mut jobs = Jobs::new(&data);
	let mut processes = Processes::new(&data);
	jobs.queue(&data, Kind::Build, "compiler".into(), "dev".into())
		.unwrap();
	jobs.queue(&data, Kind::Test, "compiler".into(), "dev".into())
		.unwrap();
	for _ in 0..400 {
		processes.poll();
		for mut entry in processes.output() {
			jobs.enrich(&mut entry);
		}
		jobs.tick(&mut processes, &data.join("logs"));
		if jobs
			.runs
			.iter()
			.all(|r| !matches!(r.state, Status::Queued | Status::Running))
		{
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(25));
	}
	assert_eq!(jobs.runs[1].state, Status::Succeeded);
	assert_eq!(jobs.runs[0].state, Status::Failed);
	assert!(jobs.runs[1].ended_ms <= jobs.runs[0].started_ms);
	for (kind, source, expected) in [
		(
			Kind::Test,
			"#[test] fn passing(){assert_eq!(2+2,4);}",
			Status::Succeeded,
		),
		(
			Kind::Build,
			"pub fn broken() { let value: u32 = \"wrong\"; }",
			Status::Failed,
		),
	] {
		std::fs::write(data.join("src/lib.rs"), source).unwrap();
		jobs.queue(&data, kind, "compiler".into(), "dev".into())
			.unwrap();
		for _ in 0..400 {
			processes.poll();
			for mut entry in processes.output() {
				jobs.enrich(&mut entry);
			}
			jobs.tick(&mut processes, &data.join("logs"));
			if !matches!(jobs.runs[0].state, Status::Queued | Status::Running) {
				break;
			}
			std::thread::sleep(std::time::Duration::from_millis(25));
		}
		assert_eq!(jobs.runs[0].state, expected);
	}
}
#[cfg(windows)]
#[test]
fn active_cancellation_and_compiler_span() {
	let data = crate::tests::test_directory("cancel");
	std::fs::create_dir_all(&data).unwrap();
	let mut jobs = Jobs::new(&data);
	let mut processes = Processes::new(&data);
	let run = jobs
		.queue(&data, Kind::Build, "compiler".into(), "dev".into())
		.unwrap();
	let mut command = Command::new("powershell.exe");
	command.args([
		"-NoProfile",
		"-NonInteractive",
		"-Command",
		"Start-Sleep -Seconds 30",
	]);
	let process = processes
		.start_command(&run.id, command, &data, &data)
		.unwrap();
	jobs.runs[0].state = Status::Running;
	jobs.runs[0].process_id = Some(process.id);
	jobs.cancel(&run.id, &mut processes).unwrap();
	jobs.tick(&mut processes, &data);
	assert_eq!(jobs.runs[0].state, Status::Cancelled);
	let mut entry: LogEntry = serde_json::from_value(serde_json::json!({"schema_version":1,"session_id":"fixture","sequence":1,"timestamp_ms":0,"application":"cargo","component":"process-output","process_id":1,"thread":"reader","level":"INFO","message":"","file":null,"line":null,"module":null,"error":null,"stack_trace":null,"attributes":{},"correlation":{"operation_id":run.id,"project_id":null,"build_id":null,"test_run_id":null}})).unwrap();
	entry.message = serde_json::json!({"reason":"compiler-message","message":{"level":"error","message":"mismatched types","spans":[{"is_primary":true,"file_name":"src/lib.rs","line_start":7}]}}).to_string();
	jobs.enrich(&mut entry);
	assert_eq!(entry.level, Level::Error);
	assert_eq!(entry.line, Some(7));
	assert_eq!(entry.file.as_deref(), Some("src/lib.rs"));
}
