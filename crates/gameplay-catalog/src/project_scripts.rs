//! Build and prepare an immutable set of project Rust scripts for one Play run.

use std::{
	collections::BTreeMap,
	fs::{self, File},
	io::Read,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	sync::{Arc, OnceLock},
	time::{Duration, Instant},
};

use bridge::script_assets;
use engine_core::{
	asset_catalog::{AssetId, AssetKind},
	components::{
		ParticleBurstSettings, ParticleEmitterSettings, ScriptDescriptor, ScriptProperty,
		ScriptSettings,
	},
	simulation_session::{
		ObjectScript, ObjectScriptAction, ObjectScriptCommands, ObjectScriptContext,
		ProjectScriptRegistration, ProjectileHit,
	},
};
use engine_script_runtime::{
	Hook, ObjectTransform, ScriptCommand, ScriptContext, ScriptModule, ScriptState, Setting,
};

#[derive(Clone)]
pub enum ScriptSource {
	Rust,
	Packaged,
	Auto,
	Prepared(Arc<BTreeMap<PathBuf, Vec<u8>>>),
}

pub(crate) struct ProjectScripts {
	root: PathBuf,
	source: ScriptSource,
	registrations: OnceLock<Result<Vec<ProjectScriptRegistration>, String>>,
}

impl ProjectScripts {
	pub(crate) fn new(root: PathBuf, source: ScriptSource) -> Self {
		Self {
			root,
			source,
			registrations: OnceLock::new(),
		}
	}

	pub(crate) fn registrations(&self) -> Result<Vec<ProjectScriptRegistration>, String> {
		self.registrations.get_or_init(|| self.load()).clone()
	}

	fn load(&self) -> Result<Vec<ProjectScriptRegistration>, String> {
		let assets = script_assets::catalog(&self.root)?;
		let mut entries = Vec::new();
		for asset in assets
			.assets
			.iter()
			.filter(|asset| asset.kind == AssetKind::RustScript)
		{
			let artifact = script_assets::artifact_path(&asset.id);
			if let ScriptSource::Prepared(files) = &self.source
				&& !files.contains_key(Path::new(&artifact))
			{
				continue;
			}
			let packaged = matches!(self.source, ScriptSource::Packaged)
				|| matches!(self.source, ScriptSource::Auto)
				&& self.root.join("script-compiled").is_dir();
			let text = if packaged {
				script_assets::read_packaged_source(&self.root, asset)?
			} else {
				script_assets::read_source(&self.root, asset)?
			};
			let metadata = script_assets::metadata(asset, &text)?;
			let bytes = match &self.source {
				ScriptSource::Rust => compile(&asset.path, &text)?,
				ScriptSource::Packaged => script_assets::read_artifact(&self.root, &asset.id)?,
				ScriptSource::Auto if self.root.join("script-compiled").is_dir() => {
					script_assets::read_artifact(&self.root, &asset.id)?
				}
				ScriptSource::Auto => compile(&asset.path, &text)?,
				ScriptSource::Prepared(files) => files[Path::new(&artifact)].clone(),
			};
			let module =
				ScriptModule::load(&bytes).map_err(|error| format!("{}: {error}", asset.path))?;
			entries.push(registration(metadata, module));
		}
		Ok(entries)
	}
}

pub fn descriptors(root: &Path) -> Result<Vec<ScriptDescriptor>, String> {
	let catalog = script_assets::catalog(root)?;
	catalog
		.assets
		.iter()
		.filter(|asset| asset.kind == AssetKind::RustScript)
		.map(|asset| script_assets::descriptor(asset, &script_assets::read_source(root, asset)?))
		.collect()
}

pub fn compile(path: &str, text: &str) -> Result<Vec<u8>, String> {
	let scratch = Scratch::new()?;
	fs::write(scratch.0.join("sdk.rs"), engine_script_runtime::SDK_SOURCE)
		.map_err(|error| error.to_string())?;
	fs::write(scratch.0.join("script.rs"), text).map_err(|error| error.to_string())?;
	let rustc = |arguments: &[&str]| {
		let mut command = Command::new("rustc");
		command
			.current_dir(&scratch.0)
			.args([
				"--edition=2024",
				"--target=wasm32-unknown-unknown",
				"--error-format=short",
				"-C",
				"panic=abort",
			])
			.args(arguments);
		#[cfg(windows)]
		{
			use std::os::windows::process::CommandExt;
			command.creation_flags(0x08000000);
		}
		run_rustc(command, &scratch.0.join("diagnostics.txt"), path)
	};
	rustc(&[
		"--crate-name",
		"engine_script_sdk",
		"--crate-type=rlib",
		"-O",
		"sdk.rs",
		"-o",
		"libengine_script_sdk.rlib",
	])?;
	rustc(&[
		"--crate-name",
		"project_script",
		"--crate-type=cdylib",
		"-O",
		"--extern",
		"engine_script_sdk=libengine_script_sdk.rlib",
		"script.rs",
		"-o",
		"script.wasm",
	])?;
	let bytes = fs::read(scratch.0.join("script.wasm")).map_err(|error| error.to_string())?;
	if bytes.len() > script_assets::MAX_SCRIPT_ARTIFACT_BYTES {
		return Err(format!(
			"{path}: Compiled script exceeds the 8 MiB artifact limit"
		));
	}
	Ok(bytes)
}

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Result<Self, String> {
		let folder = std::env::temp_dir().join(format!(
			"game-engine-script-{}",
			AssetId::generate().as_str()
		));
		fs::create_dir(&folder).map_err(|error| error.to_string())?;
		Ok(Self(folder))
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn run_rustc(mut command: Command, log: &Path, asset: &str) -> Result<(), String> {
	let stderr = File::create(log).map_err(|error| error.to_string())?;
	let mut child = command
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::from(stderr))
		.spawn()
		.map_err(|error| format!("{asset}: Could not start rustc: {error}"))?;
	let deadline = Instant::now() + Duration::from_secs(30);
	let status = loop {
		if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
			break status;
		}
		if Instant::now() >= deadline {
			let _ = child.kill();
			let _ = child.wait();
			return Err(format!(
				"{asset}: Script compilation took longer than 30 seconds"
			));
		}
		std::thread::sleep(Duration::from_millis(20));
	};
	if status.success() {
		return Ok(());
	}
	let mut message = Vec::new();
	File::open(log)
		.map_err(|error| error.to_string())?
		.take(16 * 1024)
		.read_to_end(&mut message)
		.map_err(|error| error.to_string())?;
	Err(format!(
		"{asset}: Rust script compilation failed\n{}",
		String::from_utf8_lossy(&message).replace("script.rs:", &format!("{asset}:"))
	))
}

fn registration(
	metadata: script_assets::ScriptMetadata,
	module: ScriptModule,
) -> ProjectScriptRegistration {
	let captured = metadata.descriptor.clone();
	let sprites: Arc<[AssetId]> = metadata.particle_sprites.into();
	ProjectScriptRegistration {
		descriptor: metadata.descriptor,
		instantiate: Arc::new(move |saved: &ScriptSettings| {
			let settings = captured
				.fields
				.iter()
				.filter_map(|field| {
					let key = field.key();
					let value = match captured.property(saved, key)? {
						ScriptProperty::Boolean(value) => Setting::Boolean(value),
						ScriptProperty::Number(value) => Setting::Number(value),
						ScriptProperty::Text(value) => Setting::Text(value),
					};
					Some((key.to_owned(), value))
				})
				.collect();
			Ok(Box::new(Instance {
				module: module.clone(),
				state: ScriptState::default(),
				settings: Arc::new(settings),
				sprites: sprites.clone(),
			}))
		}),
	}
}

#[derive(Clone)]
struct Instance {
	module: ScriptModule,
	state: ScriptState,
	settings: Arc<BTreeMap<String, Setting>>,
	sprites: Arc<[AssetId]>,
}

impl Instance {
	fn invoke(
		&mut self,
		hook: Hook<'_>,
		context: &ObjectScriptContext<'_>,
		output: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		for command in self.call(hook, context)? {
			match command {
				ScriptCommand::SetLocalYaw(value) => output.set_local_yaw(value)?,
				ScriptCommand::SetAnimatorBool { parameter, value } => {
					output.set_animator_bool(parameter, value)?
				}
				ScriptCommand::EmitParticleBurst { effect, position } => {
					let id = AssetId::new(effect)?;
					if !self.sprites.contains(&id) {
						return Err(
							"Script must declare the sprite in particle_sprites before emitting it"
								.into(),
						);
					}
					output.emit_particle_burst(
						position,
						ParticleBurstSettings {
							count: 12,
							settings: ParticleEmitterSettings {
								sprite_asset_id: Some(id),
								..Default::default()
							},
						},
					)?;
				}
				ScriptCommand::DestroyOwner => output.destroy_owner()?,
			}
		}
		Ok(())
	}

	fn call(
		&mut self,
		hook: Hook<'_>,
		context: &ObjectScriptContext<'_>,
	) -> Result<Vec<ScriptCommand>, String> {
		let references = context
			.reference_transforms()
			.map(|(name, transform)| {
				(
					name.to_owned(),
					ObjectTransform {
						position: transform.position,
						rotation: transform.rotation,
						scale: transform.scale,
					},
				)
			})
			.collect();
		self.module.invoke(
			&mut self.state,
			hook,
			&ScriptContext {
				tick: context.tick(),
				step_seconds: context.step_seconds(),
				owner: context.owner().key.as_str(),
				owner_yaw: context.owner_yaw()?,
				settings: &self.settings,
				references: &references,
			},
		)
	}
}

impl ObjectScript for Instance {
	fn clone_instance(&self) -> Box<dyn ObjectScript> {
		Box::new(self.clone())
	}
	fn on_start(
		&mut self,
		context: &ObjectScriptContext<'_>,
		output: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.invoke(Hook::Start, context, output)
	}
	fn on_fixed_update(
		&mut self,
		context: &ObjectScriptContext<'_>,
		output: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.invoke(Hook::FixedUpdate, context, output)
	}
	fn on_action(
		&mut self,
		context: &ObjectScriptContext<'_>,
		input: ObjectScriptAction<'_>,
		output: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.invoke(
			Hook::Action {
				id: input.id,
				pressed: input.pressed,
			},
			context,
			output,
		)
	}
	fn on_hit(
		&mut self,
		context: &ObjectScriptContext<'_>,
		hit: &ProjectileHit,
		output: &mut ObjectScriptCommands<'_, '_>,
	) -> Result<(), String> {
		self.invoke(
			Hook::Hit {
				position: hit.position,
				normal: hit.normal,
				target: hit.target.key.as_str(),
			},
			context,
			output,
		)
	}
	fn on_destroy(&mut self, context: &ObjectScriptContext<'_>) -> Result<(), String> {
		self.call(Hook::Destroy, context).map(|_| ())
	}
}
