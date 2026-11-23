//! Select project resources according to how the native Editor was built.

use std::path::{Path, PathBuf};
use tauri::{Manager, WebviewWindow};

const BUNDLED_PLAYER: &str = "release/player/game.exe";
const BUNDLED_SAMPLE: &str = "release/samples/cinder-oath/project.json";
const BUNDLED_AGENT_ROOT: &str = "release/agent";
const BUNDLED_AGENT_WORKER: &str = "authoring-agent-worker.exe";

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

/// The standalone worker is resolved only under the Editor's installed resource root.
pub(crate) struct AgentResources {
	pub(crate) root: PathBuf,
	pub(crate) executable: PathBuf,
}

impl AgentResources {
	pub(crate) fn for_window(window: &WebviewWindow) -> Result<Self, String> {
		let resource_dir = window
			.path()
			.resource_dir()
			.map_err(|error| error.to_string())?;
		Self::from_root(&resource_dir)
	}

	fn from_root(resource_root: &Path) -> Result<Self, String> {
		let root = bundled_directory(resource_root, BUNDLED_AGENT_ROOT)?;
		let executable = bundled_file(&root, BUNDLED_AGENT_WORKER, "Agent worker")?;
		bundled_file(&root, "bundle.json", "Agent profile")?;
		bundled_directory(&root, "engine-help")?;
		Ok(Self { root, executable })
	}
}

fn bundled_directory(root: &Path, relative: &str) -> Result<PathBuf, String> {
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	let candidate = root.join(relative);
	let directory = candidate
		.canonicalize()
		.map_err(|error| error.to_string())?;
	if directory != candidate || !directory.is_dir() {
		return Err("The bundled Agent directory is not inside its approved resource root".into());
	}
	Ok(directory)
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
