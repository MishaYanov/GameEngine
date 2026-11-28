//! Typed game APIs for project Rust scripts. The compiled module exchanges bounded
//! data with the native engine at each simulation hook.

use std::collections::BTreeMap;

#[doc(hidden)]
pub const ABI_VERSION: i32 = 1;
#[doc(hidden)]
pub const MAX_PACKET_BYTES: usize = 32 * 1024;
#[doc(hidden)]
pub const MAX_STATE_BYTES: usize = 4096;
#[doc(hidden)]
pub const MAX_COMMANDS: u32 = 32;

#[derive(Clone, Copy, Debug)]
pub struct InputAction<'a> {
	pub id: &'a str,
	pub pressed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit<'a> {
	pub position: [f32; 3],
	pub normal: [f32; 3],
	pub target: &'a str,
}

/// Per-attachment state. Encode mutable fields explicitly so the engine can copy
/// and roll back a candidate simulation step. Stateless scripts use the defaults.
pub trait Script: Default {
	fn on_start(&mut self, _context: &mut Context<'_>) {}
	fn on_fixed_update(&mut self, _context: &mut Context<'_>) {}
	fn on_action(&mut self, _context: &mut Context<'_>, _action: InputAction<'_>) {}
	fn on_hit(&mut self, _context: &mut Context<'_>, _hit: Hit<'_>) {}
	fn on_destroy(&mut self, _context: &mut Context<'_>) {}
	fn read_state(&mut self, _state: &[u8]) -> Result<(), String> {
		Ok(())
	}
	fn write_state(&self) -> Vec<u8> {
		Vec::new()
	}
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectTransform {
	pub position: [f32; 3],
	pub rotation: [f32; 3],
	pub scale: [f32; 3],
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
enum Setting<'a> {
	Boolean(bool),
	Number(f32),
	Text(&'a str),
}

pub struct Context<'a> {
	tick: u64,
	step_seconds: f32,
	owner: &'a str,
	owner_yaw: f32,
	settings: BTreeMap<&'a str, Setting<'a>>,
	references: BTreeMap<&'a str, ObjectTransform>,
	commands: PacketWriter,
	count: u32,
}

impl<'a> Context<'a> {
	pub fn tick(&self) -> u64 {
		self.tick
	}
	pub fn step_seconds(&self) -> f32 {
		self.step_seconds
	}
	pub fn game_seconds(&self) -> f64 {
		self.tick as f64 * f64::from(self.step_seconds)
	}
	pub fn owner(&self) -> &str {
		self.owner
	}
	pub fn owner_yaw(&self) -> f32 {
		self.owner_yaw
	}
	pub fn number(&self, key: &str) -> Option<f32> {
		match self.settings.get(key) {
			Some(Setting::Number(value)) => Some(*value),
			_ => None,
		}
	}
	pub fn boolean(&self, key: &str) -> Option<bool> {
		match self.settings.get(key) {
			Some(Setting::Boolean(value)) => Some(*value),
			_ => None,
		}
	}
	pub fn text(&self, key: &str) -> Option<&'a str> {
		match self.settings.get(key) {
			Some(Setting::Text(value)) => Some(*value),
			_ => None,
		}
	}
	pub fn reference_transform(&self, key: &str) -> Option<ObjectTransform> {
		self.references.get(key).copied()
	}
	pub fn set_local_yaw(&mut self, radians: f32) {
		if self.begin_command(0) {
			self.commands.float(radians);
		}
	}
	pub fn set_animator_bool(&mut self, parameter: &str, value: bool) {
		if self.begin_command(1) {
			self.commands.text(parameter);
			self.commands.byte(u8::from(value));
		}
	}
	pub fn emit_particle_burst(&mut self, sprite: &str, position: [f32; 3]) {
		if self.begin_command(2) {
			self.commands.text(sprite);
			self.commands.vector(position);
		}
	}
	pub fn destroy_owner(&mut self) {
		self.begin_command(3);
	}
	fn begin_command(&mut self, command: u8) -> bool {
		self.count += 1;
		if self.count > MAX_COMMANDS {
			return false;
		}
		self.commands.byte(command);
		true
	}
}

#[doc(hidden)]
#[derive(Default)]
pub struct PacketWriter(pub Vec<u8>);

impl PacketWriter {
	pub fn byte(&mut self, value: u8) {
		self.0.push(value);
	}
	pub fn integer(&mut self, value: u32) {
		self.0.extend(value.to_le_bytes());
	}
	pub fn long(&mut self, value: u64) {
		self.0.extend(value.to_le_bytes());
	}
	pub fn float(&mut self, value: f32) {
		self.integer(value.to_bits());
	}
	pub fn vector(&mut self, value: [f32; 3]) {
		value.into_iter().for_each(|value| self.float(value));
	}
	pub fn bytes(&mut self, value: &[u8]) {
		self.integer(value.len() as u32);
		self.0.extend(value);
	}
	pub fn text(&mut self, value: &str) {
		self.bytes(value.as_bytes());
	}
}

#[doc(hidden)]
pub struct PacketReader<'a>(pub &'a [u8]);

impl<'a> PacketReader<'a> {
	pub fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
		let (value, rest) = self
			.0
			.split_at_checked(size)
			.ok_or("Truncated script packet")?;
		self.0 = rest;
		Ok(value)
	}
	pub fn byte(&mut self) -> Result<u8, String> {
		Ok(self.take(1)?[0])
	}
	pub fn integer(&mut self) -> Result<u32, String> {
		Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
	}
	pub fn long(&mut self) -> Result<u64, String> {
		Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
	}
	pub fn float(&mut self) -> Result<f32, String> {
		let value = f32::from_bits(self.integer()?);
		value
			.is_finite()
			.then_some(value)
			.ok_or_else(|| "Script requires a finite number".into())
	}
	pub fn vector(&mut self) -> Result<[f32; 3], String> {
		Ok([self.float()?, self.float()?, self.float()?])
	}
	pub fn bytes(&mut self, limit: usize) -> Result<&'a [u8], String> {
		let size = self.integer()? as usize;
		if size > limit {
			return Err("Script packet field exceeds its limit".into());
		}
		self.take(size)
	}
	pub fn text(&mut self, limit: usize) -> Result<&'a str, String> {
		std::str::from_utf8(self.bytes(limit)?).map_err(|error| error.to_string())
	}
	pub fn boolean(&mut self) -> Result<bool, String> {
		match self.byte()? {
			0 => Ok(false),
			1 => Ok(true),
			_ => Err("Invalid script Boolean".into()),
		}
	}
}

/// Export one behavior type from a project `.rs` asset.
#[macro_export]
macro_rules! export_script {
	($script:ty) => {
		#[unsafe(no_mangle)]
		pub extern "C" fn game_engine_script_version() -> i32 {
			$crate::ABI_VERSION
		}
		#[unsafe(no_mangle)]
		pub extern "C" fn game_engine_script_call() -> i32 {
			$crate::dispatch::<$script>()
		}
	};
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "game_engine")]
unsafe extern "C" {
	fn read_input(destination: *mut u8, capacity: usize) -> i32;
	fn write_output(source: *const u8, length: usize) -> i32;
}

#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub fn dispatch<S: Script>() -> i32 {
	// SAFETY: a zero-capacity query only asks the host for the input size.
	let size = unsafe { read_input(std::ptr::null_mut(), 0) };
	if size <= 0 || size as usize > MAX_PACKET_BYTES {
		return -1;
	}
	let mut input = vec![0; size as usize];
	// SAFETY: the host validates the memory bounds and only writes into this allocation.
	let bytes = unsafe { read_input(input.as_mut_ptr(), input.len()) };
	if bytes != size {
		return -1;
	}
	let output = encode_result(run::<S>(&input[..bytes as usize]));
	// SAFETY: the host copies these initialized bytes synchronously before returning.
	unsafe { write_output(output.as_ptr(), output.len()) }
}

#[cfg(target_arch = "wasm32")]
fn encode_result(result: Result<PacketWriter, String>) -> Vec<u8> {
	match result {
		Ok(out) => out.0,
		Err(error) => {
			let mut packet = PacketWriter::default();
			packet.byte(1);
			packet.text(&error.chars().take(1024).collect::<String>());
			packet.0
		}
	}
}

#[cfg(target_arch = "wasm32")]
fn run<S: Script>(input: &[u8]) -> Result<PacketWriter, String> {
	let mut input = PacketReader(input);
	let hook = input.byte()?;
	let tick = input.long()?;
	let step_seconds = input.float()?;
	let owner = input.text(256)?;
	let owner_yaw = input.float()?;
	let mut settings = BTreeMap::new();
	let count = input.integer()?;
	if count > 32 {
		return Err("Script setting count exceeds its limit".into());
	}
	for _ in 0..count {
		let key = input.text(64)?;
		let setting = match input.byte()? {
			0 => Setting::Boolean(input.boolean()?),
			1 => Setting::Number(input.float()?),
			2 => Setting::Text(input.text(256)?),
			_ => return Err("Invalid script setting type".into()),
		};
		settings.insert(key, setting);
	}
	let mut references = BTreeMap::new();
	let count = input.integer()?;
	if count > 32 {
		return Err("Script reference count exceeds its limit".into());
	}
	for _ in 0..count {
		let key = input.text(64)?;
		references.insert(
			key,
			ObjectTransform {
				position: input.vector()?,
				rotation: input.vector()?,
				scale: input.vector()?,
			},
		);
	}
	let mut instance = S::default();
	instance.read_state(input.bytes(MAX_STATE_BYTES)?)?;
	let mut context = Context {
		tick,
		step_seconds,
		owner,
		owner_yaw,
		settings,
		references,
		commands: PacketWriter::default(),
		count: 0,
	};
	match hook {
		0 => instance.on_start(&mut context),
		1 => instance.on_fixed_update(&mut context),
		2 => instance.on_action(
			&mut context,
			InputAction {
				id: input.text(128)?,
				pressed: input.boolean()?,
			},
		),
		3 => instance.on_hit(
			&mut context,
			Hit {
				position: input.vector()?,
				normal: input.vector()?,
				target: input.text(256)?,
			},
		),
		4 => instance.on_destroy(&mut context),
		_ => return Err("Unknown script hook".into()),
	}
	let state = instance.write_state();
	if state.len() > MAX_STATE_BYTES || context.count > MAX_COMMANDS {
		return Err("Script state or command budget exceeded".into());
	}
	let mut out = PacketWriter::default();
	out.byte(0);
	out.bytes(&state);
	out.integer(context.count);
	out.0.extend(context.commands.0);
	Ok(out)
}
