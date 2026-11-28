//! Run Rust-authored WebAssembly with bounded hook execution and explicit state.

use std::{collections::BTreeMap, sync::Arc};

pub use engine_script_sdk::ObjectTransform;
use engine_script_sdk::{
	ABI_VERSION, MAX_COMMANDS, MAX_PACKET_BYTES, MAX_STATE_BYTES, PacketReader, PacketWriter,
};
use wasmi::{
	Caller, Config, Engine, Extern, Instance, Linker, Module, Store, StoreLimits,
	StoreLimitsBuilder,
};

pub const SDK_SOURCE: &str = include_str!("../../script-sdk/src/lib.rs");
const MAX_MODULE_BYTES: usize = 8 * 1024 * 1024;
const MAX_MEMORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_FUEL: u64 = 1_000_000;

pub enum Hook<'a> {
	Start,
	FixedUpdate,
	Action {
		id: &'a str,
		pressed: bool,
	},
	Hit {
		position: [f32; 3],
		normal: [f32; 3],
		target: &'a str,
	},
	Destroy,
}

#[derive(Clone, Debug)]
pub enum Setting {
	Boolean(bool),
	Number(f32),
	Text(String),
}

pub struct ScriptContext<'a> {
	pub tick: u64,
	pub step_seconds: f32,
	pub owner: &'a str,
	pub owner_yaw: f32,
	pub settings: &'a BTreeMap<String, Setting>,
	pub references: &'a BTreeMap<String, ObjectTransform>,
}

#[derive(Debug, PartialEq)]
pub enum ScriptCommand {
	SetLocalYaw(f32),
	SetAnimatorBool { parameter: String, value: bool },
	EmitParticleBurst { effect: String, position: [f32; 3] },
	DestroyOwner,
}

#[derive(Clone, Default)]
pub struct ScriptState(Vec<u8>);

#[derive(Clone)]
pub struct ScriptModule(Arc<Program>);

struct Program {
	engine: Engine,
	module: Module,
	linker: Linker<Exchange>,
}

impl ScriptModule {
	pub fn load(wasm: &[u8]) -> Result<Self, String> {
		if wasm.len() > MAX_MODULE_BYTES {
			return Err("Script module exceeds 8 MiB".into());
		}
		let mut settings = Config::default();
		settings.consume_fuel(true);
		let engine = Engine::new(&settings);
		let module = Module::new(&engine, wasm).map_err(message)?;
		let mut linker = Linker::new(&engine);
		linker
			.func_wrap("game_engine", "read_input", read_input)
			.map_err(message)?;
		linker
			.func_wrap("game_engine", "write_output", write_output)
			.map_err(message)?;
		let program = Program {
			engine,
			module,
			linker,
		};
		let (mut store, instance) = program.instantiate(Vec::new())?;
		let version = instance
			.get_typed_func::<(), i32>(&store, "game_engine_script_version")
			.map_err(message)?;
		instance
			.get_typed_func::<(), i32>(&store, "game_engine_script_call")
			.map_err(message)?;
		if version.call(&mut store, ()).map_err(message)? != ABI_VERSION {
			return Err("Script was built for a different engine scripting API".into());
		}
		Ok(Self(Arc::new(program)))
	}

	pub fn invoke(
		&self,
		state: &mut ScriptState,
		hook: Hook<'_>,
		context: &ScriptContext<'_>,
	) -> Result<Vec<ScriptCommand>, String> {
		let input = request(state, hook, context)?;
		let (mut store, instance) = self.0.instantiate(input)?;
		let run = instance
			.get_typed_func::<(), i32>(&store, "game_engine_script_call")
			.map_err(message)?;
		if run.call(&mut store, ()).map_err(message)? != 0 {
			return Err("Script hook returned an invalid response".into());
		}
		let output = store
			.data()
			.output
			.as_deref()
			.ok_or("Script hook did not return a response")?;
		response(state, output)
	}
}

impl Program {
	fn instantiate(&self, input: Vec<u8>) -> Result<(Store<Exchange>, Instance), String> {
		let limits = StoreLimitsBuilder::new()
			.memory_size(MAX_MEMORY_BYTES)
			.table_elements(4096)
			.instances(1)
			.memories(1)
			.tables(1)
			.trap_on_grow_failure(true)
			.build();
		let mut store = Store::new(
			&self.engine,
			Exchange {
				input,
				output: None,
				limits,
			},
		);
		store.limiter(|exchange| &mut exchange.limits);
		store.set_fuel(MAX_FUEL).map_err(message)?;
		let instance = self
			.linker
			.instantiate_and_start(&mut store, &self.module)
			.map_err(message)?;
		Ok((store, instance))
	}
}

struct Exchange {
	input: Vec<u8>,
	output: Option<Vec<u8>>,
	limits: StoreLimits,
}

fn read_input(mut caller: Caller<'_, Exchange>, destination: i32, capacity: i32) -> i32 {
	let size = caller.data().input.len();
	if destination == 0 && capacity == 0 {
		return size as i32;
	}
	let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
		return -1;
	};
	let Ok(capacity) = usize::try_from(capacity) else {
		return -1;
	};
	if size > capacity {
		return -1;
	}
	let input = std::mem::take(&mut caller.data_mut().input);
	let result = memory.write(&mut caller, destination as u32 as usize, &input);
	caller.data_mut().input = input;
	if result.is_ok() { size as i32 } else { -1 }
}

fn write_output(mut caller: Caller<'_, Exchange>, source: i32, length: i32) -> i32 {
	let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
		return -1;
	};
	let Ok(length) = usize::try_from(length) else {
		return -1;
	};
	if length > MAX_PACKET_BYTES || caller.data().output.is_some() {
		return -1;
	}
	let mut output = vec![0; length];
	if memory
		.read(&caller, source as u32 as usize, &mut output)
		.is_err()
	{
		return -1;
	}
	caller.data_mut().output = Some(output);
	0
}

fn request(
	state: &ScriptState,
	hook: Hook<'_>,
	context: &ScriptContext<'_>,
) -> Result<Vec<u8>, String> {
	if context.settings.len() > 32 || context.references.len() > 32 {
		return Err("Script context exceeds its settings or reference budget".into());
	}
	let mut data = PacketWriter::default();
	data.byte(match &hook {
		Hook::Start => 0,
		Hook::FixedUpdate => 1,
		Hook::Action { .. } => 2,
		Hook::Hit { .. } => 3,
		Hook::Destroy => 4,
	});
	data.long(context.tick);
	data.float(context.step_seconds);
	data.text(context.owner);
	data.float(context.owner_yaw);
	data.integer(context.settings.len() as u32);
	for (key, setting) in context.settings {
		data.text(key);
		match setting {
			Setting::Boolean(value) => {
				data.byte(0);
				data.byte(u8::from(*value));
			}
			Setting::Number(value) => {
				data.byte(1);
				data.float(*value);
			}
			Setting::Text(value) => {
				data.byte(2);
				data.text(value);
			}
		}
	}
	data.integer(context.references.len() as u32);
	for (key, transform) in context.references {
		data.text(key);
		data.vector(transform.position);
		data.vector(transform.rotation);
		data.vector(transform.scale);
	}
	data.bytes(&state.0);
	match hook {
		Hook::Action { id, pressed } => {
			data.text(id);
			data.byte(u8::from(pressed));
		}
		Hook::Hit {
			position,
			normal,
			target,
		} => {
			data.vector(position);
			data.vector(normal);
			data.text(target);
		}
		_ => {}
	}
	if data.0.len() > MAX_PACKET_BYTES {
		return Err("Script hook input exceeds its byte budget".into());
	}
	Ok(data.0)
}

fn response(state: &mut ScriptState, output: &[u8]) -> Result<Vec<ScriptCommand>, String> {
	let mut output = PacketReader(output);
	if output.byte()? != 0 {
		return Err(output.text(4096)?.to_owned());
	}
	let candidate = output.bytes(MAX_STATE_BYTES)?;
	let count = output.integer()?;
	if count > MAX_COMMANDS {
		return Err("Script command count exceeds its limit".into());
	}
	let mut commands = Vec::with_capacity(count as usize);
	for _ in 0..count {
		commands.push(match output.byte()? {
			0 => ScriptCommand::SetLocalYaw(output.float()?),
			1 => ScriptCommand::SetAnimatorBool {
				parameter: output.text(64)?.to_owned(),
				value: output.boolean()?,
			},
			2 => ScriptCommand::EmitParticleBurst {
				effect: output.text(64)?.to_owned(),
				position: output.vector()?,
			},
			3 => ScriptCommand::DestroyOwner,
			_ => return Err("Unknown script command".into()),
		});
	}
	if !output.0.is_empty() {
		return Err("Script response contains trailing bytes".into());
	}
	state.0.clear();
	state.0.extend_from_slice(candidate);
	Ok(commands)
}

fn message(error: impl std::fmt::Display) -> String {
	format!("WebAssembly script: {error}")
}
