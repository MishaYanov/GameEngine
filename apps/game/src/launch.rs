use std::path::PathBuf;

use courtyard_gameplay::checkpoint::{ImportIntent, ImportReview, SaveSlot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
	#[default]
	Development,
	Player,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointTransfer {
	Export {
		slot: SaveSlot,
		path: PathBuf,
	},
	Preview {
		slot: SaveSlot,
		path: PathBuf,
	},
	Commit {
		slot: SaveSlot,
		path: PathBuf,
		review: ImportReview,
		intent: ImportIntent,
	},
}

impl CheckpointTransfer {
	fn parse(
		option: &str,
		args: impl Iterator<Item=impl AsRef<str>>,
	) -> Result<Option<Self>, String> {
		if !matches!(
			option,
			"--export-slot" | "--preview-slot-import" | "--commit-slot-import"
		) {
			return Ok(None);
		}
		let mut args = args;
		let slot = match args.next().as_ref().map(AsRef::as_ref) {
			Some("1") => SaveSlot::One,
			Some("2") => SaveSlot::Two,
			Some("3") => SaveSlot::Three,
			_ => return Err("Checkpoint transfer requires a slot number from 1 to 3".into()),
		};
		let value = args
			.next()
			.ok_or("Checkpoint transfer requires a file path")?;
		let path = value.as_ref();
		if path.is_empty() || path.starts_with("--") {
			return Err("Checkpoint transfer requires a file path".into());
		}
		let path = PathBuf::from(path);
		if option != "--commit-slot-import" {
			if args.next().is_some() {
				return Err("Export and preview accept only a slot number and file path".into());
			}
			return Ok(Some(if option == "--export-slot" {
				Self::Export { slot, path }
			} else {
				Self::Preview { slot, path }
			}));
		}
		let mut archive = None;
		let mut generation = None;
		let mut intent = ImportIntent::EmptyOnly;
		while let Some(flag) = args.next() {
			match flag.as_ref() {
				"--archive-sha256" if archive.is_none() => archive = args.next(),
				"--slot-generation" if generation.is_none() => generation = args.next(),
				"--replace" if intent == ImportIntent::EmptyOnly => {
					intent = ImportIntent::ReplaceCurrent
				}
				_ => return Err("Unexpected or duplicate checkpoint import option".into()),
			}
		}
		let archive = archive.ok_or("Provide --archive-sha256 from preview")?;
		let generation = generation.ok_or("Provide --slot-generation from preview")?;
		let review = ImportReview::from_hex(archive.as_ref(), generation.as_ref())?;
		Ok(Some(Self::Commit {
			slot,
			path,
			review,
			intent,
		}))
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
	pub mode: LaunchMode,
	pub transfer: Option<CheckpointTransfer>,
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
}

impl LaunchOptions {
	/// `installed_project` is supplied by the executable's validated project
	/// marker/manifest discovery. An explicit argument always wins over the
	/// old environment switch; installed builds default to Player.
	pub fn parse(
		arguments: impl IntoIterator<Item = impl AsRef<str>>,
		installed_project: bool,
		legacy_editor_demo: bool,
	) -> Result<Self, String> {
		let mut args = arguments.into_iter().peekable();
		if let Some(argument) = args.peek()
			&& matches!(
				argument.as_ref(),
				"--export-slot" | "--preview-slot-import" | "--commit-slot-import"
			) {
			if !installed_project {
				return Err("Checkpoint transfer requires an installed Player".into());
			}
			let option = args.next().expect("peeked transfer option");
			return Ok(Self {
				mode: LaunchMode::Player,
				transfer: CheckpointTransfer::parse(option.as_ref(), args)?,
				transfer_directory: None,
				input_profile: None,
				record_playtrace: None,
				smoke_test: false,
				smoke_scene_cycles: 0,
				scene_prefetch: false,
				smoke_prefetch_probe: false,
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
                     --record-playtrace <path>, --smoke-test, --smoke-scene-cycles <count>, --scene-prefetch, --smoke-prefetch-probe, \
                     or a standalone --export-slot, --preview-slot-import, or --commit-slot-import command"
					));
				}
			}
		}

		let mode = explicit.unwrap_or_else(|| {
			if installed_project {
				LaunchMode::Player
			} else if legacy_editor_demo {
				LaunchMode::Development
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
			transfer: None,
			transfer_directory,
			input_profile,
			record_playtrace,
			smoke_test,
			smoke_scene_cycles: smoke_scene_cycles.unwrap_or(0),
			scene_prefetch,
			smoke_prefetch_probe,
		})
	}
}
