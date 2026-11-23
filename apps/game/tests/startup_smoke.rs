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

fn run_smoke(mut command: Command, mode: &str) {
	command
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
	run_smoke(Command::new(env!("CARGO_BIN_EXE_game")), "development");
	let mut player = Command::new(env!("CARGO_BIN_EXE_game"));
	player.arg("--player");
	run_smoke(player, "player");
}

#[cfg(windows)]
#[test]
#[ignore = "requires a copied Windows export and Vulkan"]
fn installed_game_launches_outside_its_package() {
	use std::{env, path::PathBuf};

	let executable = PathBuf::from(
		env::var_os("GAME_ENGINE_QA_EXPORTED_GAME").expect("provide copied game.exe"),
	)
	.canonicalize()
	.expect("resolve copied game.exe");
	let package = executable.parent().expect("installed package directory");
	assert!(package.join("project.json").is_file());
	assert!(package.join("assets").is_dir());
	let work_dir = PathBuf::from(
		env::var_os("GAME_ENGINE_QA_WORK_DIR").expect("provide an independent working directory"),
	)
	.canonicalize()
	.expect("resolve independent working directory");
	assert!(work_dir.is_dir());
	assert!(!work_dir.starts_with(package));
	assert!(!work_dir.join("project.json").exists());
	assert!(!work_dir.join("assets").exists());

	let profile =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rehearsal-arrows.json");
	let mut clean = Command::new(&executable);
	clean
		.current_dir(&work_dir)
		.env_remove("GAME_ENGINE_NATIVE_EDITOR_DEMO");
	run_smoke(clean, "player");
	let mut explicit = Command::new(&executable);
	explicit
		.current_dir(&work_dir)
		.env_remove("GAME_ENGINE_NATIVE_EDITOR_DEMO")
		.arg("--input-profile")
		.arg(profile);
	run_smoke(explicit, "player");
}
