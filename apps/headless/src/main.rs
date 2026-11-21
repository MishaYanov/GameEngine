//! CPU-only runner for an authored project and bounded command/tick replay.
use std::{
	io::{self, Write},
	process::ExitCode,
};

mod args;
mod replay;
mod report;
mod source;

use args::{HELP, Options, parse_options};
use replay::run;

fn main() -> ExitCode {
	let result = parse_options().and_then(|options| {
		if options.help {
			print!("{HELP}");
			return Ok(());
		}
		run_logged(options)
	});
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			let _ = serde_json::to_writer(
				io::stderr().lock(),
				&serde_json::json!({"error":{"code":"headless_run_failed","message":message}}),
			);
			eprintln!();
			ExitCode::FAILURE
		}
	}
}

fn run_logged(options: Options) -> Result<(), String> {
	let mut config =
		engine_logger::application_config("engine-headless").map_err(|error| error.to_string())?;
	// stdout and stderr carry machine-readable results.
	config.console = false;
	let _logging = engine_logger::init(config).map_err(|error| error.to_string())?;
	let summary = run(options).inspect_err(|_| {
		engine_logger::error!(target: "headless", "Headless execution failed: operation=run");
	})?;
	let stdout = io::stdout();
	let mut output = stdout.lock();
	serde_json::to_writer_pretty(&mut output, &summary)
		.map_err(io::Error::other)
		.and_then(|()| output.write_all(b"\n"))
		.map_err(|error| {
			engine_logger::error!(target: "headless", "Headless execution failed: operation=write_summary");
			error.to_string()
		})
}

#[cfg(test)]
#[path = "../tests/unit/binary.rs"]
mod tests;
