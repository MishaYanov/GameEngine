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
				"--input-profile" => {
					if input_profile.is_some() {
						return Err("Specify --input-profile only once".into());
					}

					let argument = args.next().ok_or("--input-profile requires a file path")?;

					let path = argument.as_ref();
					if path.is_empty() || path.starts_with("--") {
						return Err("--input-profile requires a file path".into());
					}

					input_profile = Some(PathBuf::from(path));
				}
				argument => {
					return Err(format!(
						"Unknown game argument {argument:?}; supported arguments are \
                     --development, --player, and --input-profile <path>"
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

		Ok(Self {
			mode,
			input_profile,
		})
	}
}
