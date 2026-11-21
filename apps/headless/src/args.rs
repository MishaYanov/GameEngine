use std::{collections::VecDeque, ffi::OsString, path::PathBuf};

use super::replay::MAX_TICK_CALLS;

pub(super) const HELP: &str = "engine-headless --project <project.json> [--asset-root <assets>]\n  or: engine-headless --scene <scene.json> [--asset-root <assets>]\nOptions:\n  --steps <0..100000>     Tick calls (default 60, or 0 with replay).\n  --dt <seconds>          Elapsed time per call; default project's fixed interval.\n  --replay <replay.json>  Replay v1-v4. Camera input needs v2, keyboard_input v3, and controller_input v4.\n  --budget-ms <1..300000> Budget checked between actions; default 30000.\n  --output <json>         Write summary to a new file as well as stdout.\n  --state-output <json>   Write final render scene to a new file.\n  --help                 Show help.\nProject scenes resolve beneath the asset root (default project folder/assets).\nStandalone scenes have no implicit behaviors. --steps starts Play; replay owns\nits mode transitions. Outputs must not exist. No GPU or window is loaded. Animated assets are decoded on the CPU.\nReplay asset_metadata actions supply optional CPU bounds for picking/framing.\nReplay keyboard_input uses a complete held physical key set (e.g. KeyE).\nReplay controller_input uses complete left_x, left_y, jump, and interact samples.\nNative component hashes cover lifecycle, controller and animation state; opaque\ngame-module state, scheduler state and render-only data are excluded.\nThe camera_runtime_v1 hash covers the resolved camera, orbit state and pending\nintent. Compare it across the same build and platform at equal replay boundaries.\n";

#[derive(Default)]
pub(super) struct Options {
	pub(super) project: Option<PathBuf>,
	pub(super) scene: Option<PathBuf>,
	pub(super) asset_root: Option<PathBuf>,
	pub(super) replay: Option<PathBuf>,
	pub(super) output: Option<PathBuf>,
	pub(super) state_output: Option<PathBuf>,
	pub(super) steps: Option<u32>,
	pub(super) dt: Option<f64>,
	pub(super) budget_ms: Option<u64>,
	pub(super) help: bool,
}

pub(super) fn parse_options() -> Result<Options, String> {
	let mut arguments: VecDeque<_> = std::env::args_os().skip(1).collect();
	if arguments.len() > 32 {
		return Err("Too many headless arguments".into());
	}
	let mut options = Options::default();
	while let Some(argument) = arguments.pop_front() {
		let flag = argument.to_str().ok_or("Flags must be valid text")?;
		match flag {
			"--project" => unique_path(&mut options.project, value(&mut arguments, flag)?, flag)?,
			"--scene" => unique_path(&mut options.scene, value(&mut arguments, flag)?, flag)?,
			"--asset-root" | "--assets" => {
				unique_path(&mut options.asset_root, value(&mut arguments, flag)?, flag)?
			}
			"--replay" => unique_path(&mut options.replay, value(&mut arguments, flag)?, flag)?,
			"--output" => unique_path(&mut options.output, value(&mut arguments, flag)?, flag)?,
			"--state-output" => unique_path(
				&mut options.state_output,
				value(&mut arguments, flag)?,
				flag,
			)?,
			"--steps" => {
				if options.steps.is_some() {
					return Err("--steps may be provided only once".into());
				}
				let count = text(value(&mut arguments, flag)?)?
					.parse::<u32>()
					.map_err(|_| "--steps needs an unsigned integer")?;
				if count > MAX_TICK_CALLS {
					return Err("--steps exceeds 100000".into());
				}
				options.steps = Some(count);
			}
			"--dt" => {
				if options.dt.is_some() {
					return Err("--dt may be provided only once".into());
				}
				let elapsed = text(value(&mut arguments, flag)?)?
					.parse::<f64>()
					.map_err(|_| "--dt needs seconds as a number")?;
				validate_elapsed(elapsed)?;
				options.dt = Some(elapsed);
			}
			"--budget-ms" => {
				if options.budget_ms.is_some() {
					return Err("--budget-ms may be provided only once".into());
				}
				let budget = text(value(&mut arguments, flag)?)?
					.parse::<u64>()
					.map_err(|_| "--budget-ms needs an integer")?;
				if !(1..=300_000).contains(&budget) {
					return Err("Wall-time budget must be 1..=300000 milliseconds".into());
				}
				options.budget_ms = Some(budget);
			}
			"--help" | "-h" => options.help = true,
			_ => return Err(format!("Unknown argument {flag:?}; use --help")),
		}
	}
	if !options.help && (options.project.is_some() == options.scene.is_some()) {
		return Err("Provide exactly one --project or --scene; use --help".into());
	}
	Ok(options)
}
fn value(arguments: &mut VecDeque<OsString>, flag: &str) -> Result<OsString, String> {
	arguments
		.pop_front()
		.ok_or_else(|| format!("{flag} needs a value"))
}
fn text(value: OsString) -> Result<String, String> {
	value
		.into_string()
		.map_err(|_| "Numeric options must be valid text".into())
}
fn unique_path(target: &mut Option<PathBuf>, value: OsString, flag: &str) -> Result<(), String> {
	if target.is_some() {
		return Err(format!("{flag} may be provided only once"));
	}
	*target = Some(PathBuf::from(value));
	Ok(())
}
pub(super) fn validate_elapsed(elapsed: f64) -> Result<(), String> {
	if !elapsed.is_finite() || !(0.0..=60.0).contains(&elapsed) {
		return Err("Tick elapsed seconds must be finite and within 0..=60".into());
	}
	Ok(())
}
