/// Development preserves the learning/demo controls. Installed projects run
/// their configured gameplay as players; editor widgets belong to Angular.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
	#[default]
	Development,
	Player,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOptions {
	pub mode: LaunchMode,
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
		let mut explicit = None;
		for argument in arguments {
			let mode = match argument.as_ref() {
				"--player" => LaunchMode::Player,
				"--development" => LaunchMode::Development,
				argument => {
					return Err(format!(
						"Unknown game argument {argument:?}; supported arguments are --development and --player"
					));
				}
			};
			if explicit.is_some() {
				return Err("Specify exactly one launch mode: --development or --player".into());
			}
			explicit = Some(mode);
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
		Ok(Self { mode })
	}
}
