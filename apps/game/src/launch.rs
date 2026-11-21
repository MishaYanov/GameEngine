use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
	#[default]
	Development,
	Player,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
	pub mode: LaunchMode,
	pub input_profile: Option<PathBuf>,
	/// Publish a bounded replay from a fresh Player run on exit.
	pub record_playtrace: Option<PathBuf>,
	/// Exercise real startup and one GPU frame in a hidden window, then exit.
	pub smoke_test: bool,
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
		let mut args = arguments.into_iter();
		let mut explicit = None;
		let mut input_profile = None;
		let mut record_playtrace = None;
		let mut smoke_test = false;

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
				option @ ("--input-profile" | "--record-playtrace") => {
					let value = if option == "--input-profile" {
						&mut input_profile
					} else {
						&mut record_playtrace
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
                     --development, --player, --input-profile <path>, \
                     --record-playtrace <path>, and --smoke-test"
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

		Ok(Self {
			mode,
			input_profile,
			record_playtrace,
			smoke_test,
		})
	}
}
