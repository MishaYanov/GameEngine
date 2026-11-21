//! Bounded, opt-in native capture using the headless version 4 replay shape.

use runtime::{
	InputState, Key,
	input::{GamepadSample, InputProfile},
};
use serde::Serialize;
use std::{
	fs, io,
	path::{Path, PathBuf},
};

const SCHEMA_VERSION: u32 = 4;
const MAX_TRACE_ACTIONS: usize = 4096;
const MAX_TRACE_BYTES: usize = 1024 * 1024;
const MAX_TRACE_STEPS: u32 = 100_000;
const MAX_HELD_KEYS: u32 = 16;

/// Stores only gameplay input accepted by the active Player input owner.
pub(super) struct PlayTrace {
	path: PathBuf,
	keys: Vec<ProfileKey>,
	jump: bool,
	interact: bool,
	keyboard: u32,
	gamepad: GamepadSample,
	paused: bool,
	actions: Vec<TraceAction>,
	reserved_bytes: usize,
	steps: u32,
	full: bool,
}

impl PlayTrace {
	pub(super) fn new(
		path: PathBuf,
		authored: &InputProfile,
		effective: &InputProfile,
	) -> Result<Self, String> {
		if authored != effective {
			return Err("Playtrace requires the authored keyboard profile".into());
		}
		authored.validate()?;
		check_new_output(&path)?;

		let movement = authored.movement;
		let mut keys: Vec<ProfileKey> = Vec::new();
		for key in [
			movement.negative_x,
			movement.positive_x,
			movement.negative_y,
			movement.positive_y,
		]
		.into_iter()
		.chain(authored.actions.iter().map(|binding| binding.key))
		{
			if keys.iter().any(|entry| entry.key == key) {
				continue;
			}
			let code = key.physical_code();
			let mut bytes = code.bytes();
			if code.len() > 32
				|| !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
				|| !bytes.all(|byte| byte.is_ascii_alphanumeric())
				|| keys.iter().any(|entry| entry.code == code)
			{
				return Err("A bound key has no distinct replay-compatible physical code".into());
			}
			keys.push(ProfileKey { key, code });
		}
		keys.sort_unstable_by(|left, right| left.code.cmp(&right.code));
		let has_action = |name| authored.actions.iter().any(|binding| binding.id == name);
		let mut trace = Self {
			path,
			keys,
			jump: has_action("jump"),
			interact: has_action("interact"),
			keyboard: 0,
			gamepad: GamepadSample::default(),
			paused: false,
			actions: Vec::new(),
			reserved_bytes: 64,
			steps: 0,
			full: false,
		};
		trace.append(vec![TraceAction::Command {
			command: TraceCommand::StartPlay,
		}]);
		Ok(trace)
	}

	/// Call after the input owner has accepted the full sample, even between ticks.
	pub(super) fn record_controls(&mut self, input: &InputState, mut gamepad: GamepadSample) {
		if self.full || self.paused {
			return;
		}
		let keyboard = self
			.keys
			.iter()
			.enumerate()
			.fold(0u32, |mask, (index, entry)| {
				mask | (u32::from(input.is_held(entry.key)) << index)
			});
		if keyboard.count_ones() > MAX_HELD_KEYS {
			self.full = true;
			return;
		}
		if gamepad.movement() == [0.0; 2] {
			gamepad.left_x = 0;
			gamepad.left_y = 0;
		}
		gamepad.jump &= self.jump;
		gamepad.interact &= self.interact;
		if keyboard == self.keyboard && gamepad == self.gamepad {
			return;
		}
		let mut actions = Vec::with_capacity(2);
		if keyboard != self.keyboard {
			let keys = self
				.keys
				.iter()
				.enumerate()
				.filter(|(index, _)| keyboard & (1 << index) != 0)
				.map(|(_, entry)| entry.code.clone())
				.collect();
			actions.push(TraceAction::KeyboardInput { keys });
		}
		if gamepad != self.gamepad {
			actions.push(TraceAction::controller(gamepad));
		}
		if self.append(actions) {
			self.keyboard = keyboard;
			self.gamepad = gamepad;
		}
	}

	/// Count only accepted fixed steps, including a host's explicit drains.
	pub(super) fn record_steps(&mut self, steps: u32) {
		if self.full || self.paused || steps == 0 {
			return;
		}
		let steps = steps.min(MAX_TRACE_STEPS - self.steps);
		if let Some(TraceAction::Tick { count }) = self.actions.last_mut() {
			*count += steps;
		} else if !self.append(vec![TraceAction::Tick { count: steps }]) {
			return;
		}
		self.steps += steps;
		self.full |= self.steps == MAX_TRACE_STEPS;
	}

	/// Replay a real pause change before the next accepted input or tick.
	pub(super) fn record_paused(&mut self, paused: bool) {
		if self.paused != paused && self.append(vec![TraceAction::pause(paused)]) {
			self.paused = paused;
			self.keyboard = 0;
			self.gamepad = GamepadSample::default();
		}
	}

	/// Mirror a host focus/menu/scene boundary without leaving Play paused.
	pub(super) fn clear_controls(&mut self) {
		if self.full || self.paused {
			return;
		}
		let mut actions = Vec::with_capacity(4);
		if self.keyboard != 0 {
			actions.push(TraceAction::KeyboardInput { keys: Vec::new() });
		}
		if self.gamepad != GamepadSample::default() {
			actions.push(TraceAction::controller(GamepadSample::default()));
		}
		// v4 has no clear-input record. Toggling Pause without a step clears
		// pending button edges, including a complete press/release since the last tick.
		actions.extend([TraceAction::pause(true), TraceAction::pause(false)]);
		if self.append(actions) {
			self.keyboard = 0;
			self.gamepad = GamepadSample::default();
		}
	}

	pub(super) fn is_full(&self) -> bool {
		self.full
	}

	/// Publish the accepted prefix and never open an already occupied destination.
	pub(super) fn finish(self) -> Result<(), String> {
		let mut bytes = serde_json::to_vec(&TraceFile {
			schema_version: SCHEMA_VERSION,
			actions: &self.actions,
		})
		.map_err(|error| format!("Could not encode playtrace: {error}"))?;
		bytes.push(b'\n');
		if bytes.len() > MAX_TRACE_BYTES {
			return Err("Encoded playtrace exceeds its byte limit".into());
		}
		runtime::scene_io::save_new_bytes(&self.path, &bytes)
			.map_err(|error| format!("Could not publish playtrace: {error}"))
	}

	fn append(&mut self, actions: Vec<TraceAction>) -> bool {
		if self.full {
			return false;
		}
		let reserved_bytes = self.reserved_bytes
			+ actions
				.iter()
				.map(TraceAction::reserved_bytes)
				.sum::<usize>();
		if self.actions.len() + actions.len() > MAX_TRACE_ACTIONS
			|| reserved_bytes > MAX_TRACE_BYTES
		{
			self.full = true;
			return false;
		}
		self.actions.extend(actions);
		self.reserved_bytes = reserved_bytes;
		self.full = self.actions.len() == MAX_TRACE_ACTIONS;
		true
	}
}

struct ProfileKey {
	key: Key,
	code: String,
}

#[derive(Serialize)]
struct TraceFile<'a> {
	schema_version: u32,
	actions: &'a [TraceAction],
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TraceAction {
	Command {
		command: TraceCommand,
	},
	KeyboardInput {
		keys: Vec<String>,
	},
	ControllerInput {
		left_x: i16,
		left_y: i16,
		jump: bool,
		interact: bool,
	},
	Tick {
		count: u32,
	},
}

impl TraceAction {
	fn controller(sample: GamepadSample) -> Self {
		Self::ControllerInput {
			left_x: sample.left_x,
			left_y: sample.left_y,
			jump: sample.jump,
			interact: sample.interact,
		}
	}

	fn pause(paused: bool) -> Self {
		Self::Command {
			command: TraceCommand::SetPaused { paused },
		}
	}

	fn reserved_bytes(&self) -> usize {
		// Enough for the longest fixed-size record, integer growth after tick
		// coalescing, and delimiters. Physical codes use only ASCII alphanumerics.
		112 + match self {
			Self::KeyboardInput { keys } => keys.iter().map(|key| key.len() + 3).sum(),
			_ => 0,
		}
	}
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TraceCommand {
	StartPlay,
	SetPaused { paused: bool },
}

fn check_new_output(path: &Path) -> Result<(), String> {
	let name = path
		.file_name()
		.ok_or("Playtrace needs an output file name")?;
	#[cfg(windows)]
	{
		use std::os::windows::ffi::OsStrExt;
		if name.encode_wide().any(|unit| unit == u16::from(b':')) {
			return Err("Playtrace output cannot be a Windows alternate data stream".into());
		}
	}
	#[cfg(not(windows))]
	let _ = name;
	match fs::symlink_metadata(path) {
		Ok(_) => return Err("Playtrace output already exists".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(format!("Could not inspect playtrace output: {error}")),
	}
	let parent = path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or(Path::new("."));
	let metadata = fs::metadata(parent)
		.map_err(|error| format!("Could not inspect playtrace output directory: {error}"))?;
	if !metadata.is_dir() {
		return Err("Playtrace output parent must be a directory".into());
	}
	Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/playtrace.rs"]
mod tests;
