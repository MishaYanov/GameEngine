use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
	#[default]
	Development,
	Player,
}

/// Standalone installed-Player operations. These never create a viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPresetCommand {
	Export(PathBuf),
	Preview(PathBuf),
	Apply {
		path: PathBuf,
		source_sha256: [u8; 32],
		destination_sha256: [u8; 32],
	},
}

impl InputPresetCommand {
	fn matches(option: &str) -> bool {
		matches!(
			option,
			"--export-input-preset" | "--preview-input-preset" | "--apply-input-preset"
		)
	}

	fn parse(
		option: &str,
		mut args: impl Iterator<Item = impl AsRef<str>>,
	) -> Result<Self, String> {
		let path = args.next().ok_or("Input preset requires a file path")?;
		let path = path.as_ref();
		if path.is_empty() || path.starts_with("--") {
			return Err("Input preset requires a file path".into());
		}
		let path = PathBuf::from(path);
		if option != "--apply-input-preset" {
			if args.next().is_some() {
				return Err("Preset export and preview accept only a file path".into());
			}
			return Ok(if option == "--export-input-preset" {
				Self::Export(path)
			} else {
				Self::Preview(path)
			});
		}
		let mut source_sha256 = None;
		let mut destination_sha256 = None;
		while let Some(flag) = args.next() {
			let destination = match flag.as_ref() {
				"--source-sha256" if source_sha256.is_none() => &mut source_sha256,
				"--destination-sha256" if destination_sha256.is_none() => &mut destination_sha256,
				_ => return Err("Unexpected or duplicate input preset option".into()),
			};
			let digest = args
				.next()
				.ok_or("Expected the SHA-256 printed by preview")?;
			*destination = Some(parse_digest(digest.as_ref())?);
		}
		Ok(Self::Apply {
			path,
			source_sha256: source_sha256.ok_or("Provide --source-sha256 from preview")?,
			destination_sha256: destination_sha256
				.ok_or("Provide --destination-sha256 from preview")?,
		})
	}
}

fn parse_digest(value: &str) -> Result<[u8; 32], String> {
	if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
		return Err("Expected a 64-digit SHA-256 from preview".into());
	}
	let mut digest = [0u8; 32];
	for (index, byte) in digest.iter_mut().enumerate() {
		*byte = u8::from_str_radix(&value[index * 2..][..2], 16)
			.map_err(|_| "Invalid hexadecimal SHA-256")?;
	}
	Ok(digest)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
	pub mode: LaunchMode,
	pub input_preset: Option<InputPresetCommand>,
	pub transfer_directory: Option<PathBuf>,
	pub input_profile: Option<PathBuf>,
	/// Publish a bounded replay from a fresh Player run on exit.
	pub record_playtrace: Option<PathBuf>,
	/// Exercise real startup and one GPU frame in a hidden window, then exit.
	pub smoke_test: bool,
	/// Count of measured scene transfers in a Player smoke run.
	pub smoke_scene_cycles: u32,
	pub scene_prefetch: bool,
	pub smoke_prefetch_probe: bool,
	pub smoke_retain_recent_source: bool,
}

impl LaunchOptions {
	/// `installed_project` is supplied by the executable's validated project
	/// manifest discovery. Installed builds default to Player.
	pub fn parse(
		arguments: impl IntoIterator<Item = impl AsRef<str>>,
		installed_project: bool,
	) -> Result<Self, String> {
		let mut args = arguments.into_iter().peekable();
		if let Some(argument) = args.peek()
			&& InputPresetCommand::matches(argument.as_ref())
		{
			let option = args.next().expect("peeked input preset option");
			if !installed_project {
				return Err("Input presets require an installed Player".into());
			}
			return Ok(Self {
				mode: LaunchMode::Player,
				input_preset: Some(InputPresetCommand::parse(option.as_ref(), args)?),
				transfer_directory: None,
				input_profile: None,
				record_playtrace: None,
				smoke_test: false,
				smoke_scene_cycles: 0,
				scene_prefetch: false,
				smoke_prefetch_probe: false,
				smoke_retain_recent_source: false,
			});
		}
		let mut explicit = None;
		let mut input_profile = None;
		let mut transfer_directory = None;
		let mut record_playtrace = None;
		let mut smoke_test = false;
		let mut smoke_scene_cycles = None;
		let mut scene_prefetch = false;
		let mut smoke_prefetch_probe = false;
		let mut smoke_retain_recent_source = false;

		while let Some(argument) = args.next() {
			match argument.as_ref() {
				"--player" | "--development" => {
					let mode = if argument.as_ref() == "--player" {
						LaunchMode::Player
					} else {
						LaunchMode::Development
					};

					if explicit.is_some() {
						return Err(
							"Specify exactly one launch mode: --development or --player".into()
						);
					}

					explicit = Some(mode);
				}
				"--smoke-test" => {
					if smoke_test {
						return Err("Specify --smoke-test only once".into());
					}
					smoke_test = true;
				}
				"--smoke-prefetch-probe" if !smoke_prefetch_probe => smoke_prefetch_probe = true,
				"--smoke-prefetch-probe" => {
					return Err("Specify --smoke-prefetch-probe only once".into());
				}
				"--smoke-retain-recent-source" if !smoke_retain_recent_source => {
					smoke_retain_recent_source = true
				}
				"--smoke-retain-recent-source" => {
					return Err("Specify --smoke-retain-recent-source only once".into());
				}
				"--scene-prefetch" if !scene_prefetch => scene_prefetch = true,
				"--scene-prefetch" => return Err("Specify --scene-prefetch only once".into()),
				"--smoke-scene-cycles" => {
					if smoke_scene_cycles.is_some() {
						return Err("Specify --smoke-scene-cycles only once".into());
					}
					smoke_scene_cycles = Some(
						args.next()
							.and_then(|arg| arg.as_ref().parse::<u32>().ok())
							.filter(|count| (1..=100).contains(count))
							.ok_or("--smoke-scene-cycles requires a count from 1 to 100")?,
					);
				}
				option @ ("--input-profile" | "--record-playtrace" | "--transfer-directory") => {
					let value = match option {
						"--input-profile" => &mut input_profile,
						"--transfer-directory" => &mut transfer_directory,
						_ => &mut record_playtrace,
					};
					if value.is_some() {
						return Err(format!("Specify {option} only once"));
					}
					let argument = args
						.next()
						.ok_or_else(|| format!("{option} requires a file path"))?;
					let path = argument.as_ref();
					if path.is_empty() || path.starts_with("--") {
						return Err(format!("{option} requires a file path"));
					}
					*value = Some(PathBuf::from(path));
				}
				argument => {
					return Err(format!(
						"Unknown game argument {argument:?}; supported arguments are \
                     --development, --player, --input-profile <path>, --transfer-directory <path>, \
                     --record-playtrace <path>, --smoke-test, --smoke-scene-cycles <count>, --scene-prefetch, --smoke-prefetch-probe, --smoke-retain-recent-source, \
                     or a standalone --export-input-preset, --preview-input-preset, or --apply-input-preset command"
					));
				}
			}
		}

		let mode = explicit.unwrap_or_else(|| {
			if installed_project {
				LaunchMode::Player
			} else {
				LaunchMode::default()
			}
		});
		if record_playtrace.is_some() && (mode != LaunchMode::Player || smoke_test) {
			return Err("--record-playtrace requires a Player run without --smoke-test".into());
		}

		if smoke_scene_cycles.is_some() && (mode != LaunchMode::Player || !smoke_test) {
			return Err("--smoke-scene-cycles requires --smoke-test in Player mode".into());
		}

		if smoke_prefetch_probe
			&& (!installed_project || smoke_scene_cycles.is_none_or(|count| count < 2))
		{
			return Err("--smoke-prefetch-probe requires an installed Player and at least two smoke scene cycles".into());
		}
		if smoke_retain_recent_source
			&& (!installed_project || smoke_scene_cycles.is_none_or(|count| count < 2))
		{
			return Err("--smoke-retain-recent-source requires an installed Player and at least two smoke scene cycles".into());
		}
		if scene_prefetch && (mode != LaunchMode::Player || !installed_project) {
			return Err("--scene-prefetch requires an installed Player".into());
		}
		if transfer_directory.is_some()
			&& (!installed_project || mode != LaunchMode::Player || smoke_test)
		{
			return Err("--transfer-directory requires an interactive installed Player".into());
		}
		Ok(Self {
			mode,
			input_preset: None,
			transfer_directory,
			input_profile,
			record_playtrace,
			smoke_test,
			smoke_scene_cycles: smoke_scene_cycles.unwrap_or(0),
			scene_prefetch,
			smoke_prefetch_probe,
			smoke_retain_recent_source,
		})
	}
}
