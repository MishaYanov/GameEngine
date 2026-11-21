use super::*;

fn fixture_command(mode: &str) -> Command {
	#[cfg(windows)]
	{
		let mut command = Command::new("powershell.exe");
		command.args([
			"-NoProfile",
			"-NonInteractive",
			"-Command",
			match mode {
				"hang" => "Write-Output 'fixture started'; Start-Sleep -Seconds 30",
				"fail" => "Write-Output 'fixture failed'; exit 7",
				_ => "Write-Output 'fixture succeeded'; exit 0",
			},
		]);
		command
	}
	#[cfg(not(windows))]
	{
		let mut command = Command::new("sh");
		command.args([
			"-c",
			match mode {
				"hang" => "echo fixture; sleep 30",
				"fail" => "echo fixture; exit 7",
				_ => "echo fixture; exit 0",
			},
		]);
		command
	}
}
fn await_exit(processes: &mut Processes) {
	let started = Instant::now();
	while processes.history()[0].state == ProcessState::Running {
		processes.poll();
		assert!(started.elapsed() < Duration::from_secs(10));
		std::thread::sleep(Duration::from_millis(20));
	}
}
#[test]
fn managed_process_start_stop_restart_and_failure() {
	let fixture = Fixture::new();
	let mut processes = Processes::new(&fixture.data());
	let first = processes
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	assert!(first.pid > 0);
	assert!(
		processes
			.start_command(
				"compiler",
				fixture_command("hang"),
				&fixture.0,
				&fixture.logs()
			)
			.is_err()
	);
	processes.stop("compiler").unwrap();
	assert_eq!(processes.history()[0].state, ProcessState::Stopped);
	assert!(processes.history()[0].intentional_stop);
	let second = processes
		.start_command(
			"compiler",
			fixture_command("fail"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	assert_ne!(first.id, second.id);
	await_exit(&mut processes);
	assert_eq!(processes.history()[0].state, ProcessState::Failed);
	assert_eq!(processes.history()[0].exit_code, Some(7));
	processes
		.start_command(
			"compiler",
			fixture_command("success"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	await_exit(&mut processes);
	assert_eq!(processes.history()[0].state, ProcessState::Succeeded);
	assert!(processes.stop("unowned").is_err());
	let restored = Processes::new(&fixture.data());
	assert_eq!(restored.history().len(), 3);
}
#[test]
fn interrupted_history_is_not_adopted() {
	let fixture = Fixture::new();
	let mut owner = Processes::new(&fixture.data());
	owner
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	let mut restored = Processes::new(&fixture.data());
	assert_eq!(restored.history()[0].state, ProcessState::Interrupted);
	assert!(restored.stop("compiler").is_err());
	owner.stop("compiler").unwrap();
}

#[test]
fn process_exit_updates_session_health() {
	let fixture = Fixture::new();
	let mut service = AdminService::new(fixture.0.clone(), fixture.logs(), fixture.data());
	let run = service
		.processes
		.start_command(
			"compiler",
			fixture_command("hang"),
			&fixture.0,
			&fixture.logs(),
		)
		.unwrap();
	let mut entry = record("compiler-123-1", 1);
	entry.component = "compiler".into();
	entry.process_id = run.pid;
	entry.timestamp_ms = now_ms();
	entry.attributes = [
		("event".into(), "component_health".into()),
		("health".into(), "HEALTHY".into()),
	]
	.into();
	fixture.write(&entry);
	service.tick();
	assert_eq!(
		service
			.snapshot()
			.components
			.into_iter()
			.find(|c| c.id == "compiler")
			.unwrap()
			.health,
		HealthState::Healthy
	);
	service.processes.stop("compiler").unwrap();
	service.tick();
	assert_eq!(
		service
			.snapshot()
			.components
			.into_iter()
			.find(|c| c.id == "compiler")
			.unwrap()
			.health,
		HealthState::Stopped
	);
}
