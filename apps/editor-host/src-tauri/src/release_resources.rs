//! Select project resources according to how the native Editor was built.

use std::path::{Path, PathBuf};
use tauri::{Manager, WebviewWindow};

const BUNDLED_PLAYER: &str = "release/player/game.exe";
const BUNDLED_SAMPLE: &str = "release/samples/cinder-oath/project.json";

pub(crate) enum EditorResources {
	Development,
	Installed(PathBuf),
}

impl EditorResources {
	pub(crate) fn for_window(window: &WebviewWindow) -> Result<Self, String> {
		if cfg!(feature = "custom-protocol") {
			let root = window.path().resource_dir().map_err(|error| {
				format!("Could not find the installed Editor resources: {error}")
			})?;
			Ok(Self::Installed(root))
		} else {
			Ok(Self::Development)
		}
	}

	pub(crate) fn create_sample(&self, name: &str, destination: &str) -> Result<PathBuf, String> {
		match self {
			Self::Development => compiler::create_authoring_project(name, destination),
			Self::Installed(root) => {
				let manifest = bundled_file(root, BUNDLED_SAMPLE, "Cinder Oath sample")?;
				compiler::create_authoring_project_from_template(name, destination, &manifest)
			}
		}
	}

	/// Development may build the Player from source. An installed app must have its own Player.
	pub(crate) fn player(&self) -> Result<Option<PathBuf>, String> {
		match self {
			Self::Development => Ok(None),
			Self::Installed(root) => bundled_file(root, BUNDLED_PLAYER, "Windows Player").map(Some),
		}
	}
}

fn bundled_file(root: &Path, relative: &str, name: &str) -> Result<PathBuf, String> {
	let result = (|| {
		let root = root.canonicalize()?;
		let candidate = root.join(relative);
		let metadata = candidate.symlink_metadata()?;
		let path = candidate.canonicalize()?;
		if !metadata.is_file() || path != candidate {
			return Err(std::io::Error::other(
				"the resource path changed or is not a regular file",
			));
		}
		Ok(path)
	})();
	result.map_err(|error| {
		format!("The bundled {name} is unavailable. Repair or reinstall the Editor. {error}")
	})
}

#[cfg(test)]
#[path = "../tests/unit/tests_release_resources.rs"]
mod tests;
