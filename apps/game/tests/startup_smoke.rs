//! Spawn the actual executable: native unit-test threads have a larger stack
//! than the Windows executable main thread and missed the startup overflow.
use std::{
	io::{Read, Write},
	process::{Command, Stdio},
	thread,
	time::{Duration, Instant},
};

fn bounded_output(mut stream: impl Read) -> Vec<u8> {
	let mut captured = Vec::new();
	stream
		.by_ref()
		.take(1024 * 1024)
		.read_to_end(&mut captured)
		.unwrap();
	// Drain any excess so diagnostics cannot fill the child pipe and deadlock it.
	std::io::copy(&mut stream, &mut std::io::sink()).unwrap();
	captured
}

fn run_smoke(arguments: &[&str], mode: &str) {
	let mut command = Command::new(env!("CARGO_BIN_EXE_game"));
	command
		.args(arguments)
		.arg("--smoke-test")
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console popup.
	}
	let mut child = command.spawn().expect("start the real game executable");
	let stdout = child.stdout.take().unwrap();
	let stderr = child.stderr.take().unwrap();
	let stdout = thread::spawn(move || bounded_output(stdout));
	let stderr = thread::spawn(move || bounded_output(stderr));
	let deadline = Instant::now() + Duration::from_secs(90);
	let mut timed_out = false;
	let status = loop {
		if let Some(status) = child.try_wait().expect("poll game") {
			break status;
		}
		if Instant::now() >= deadline {
			timed_out = true;
			child
				.kill()
				.expect("stop only the smoke child after timeout");
			break child.wait().expect("reap smoke child");
		}
		thread::sleep(Duration::from_millis(20));
	};
	let stdout = stdout.join().unwrap();
	let stderr = stderr.join().unwrap();
	assert!(
		!timed_out,
		"{mode} startup timed out: {}",
		String::from_utf8_lossy(&stderr)
	);
	assert!(
		status.success(),
		"{mode} startup failed ({status}):\n{}\n{}",
		String::from_utf8_lossy(&stdout),
		String::from_utf8_lossy(&stderr)
	);
	assert!(
		String::from_utf8_lossy(&stdout).contains(&format!("GAME_STARTUP_SMOKE_OK mode={mode}")),
		"{mode} exited without completing its native frame"
	);
	std::io::stdout().write_all(&stdout).unwrap();
}

#[test]
#[ignore = "requires Vulkan and a native hidden window; launches the real executable"]
fn actual_game_main_stack_initializes_and_draws_development_and_player() {
	run_smoke(&[], "development");
	run_smoke(&["--player"], "player");
}
