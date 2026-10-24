use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ProjectOpenError {
	InvalidManifestName,
	InvalidManifest,
	ManifestIsSymlink,
	MissingRootDirectory,
	MissingAssetsDirectory,
	AssetsNotDirectory,
	AssetsNotInProjectDirectory,
	Io(std::io::Error),
}

impl fmt::Display for ProjectOpenError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::InvalidManifestName => write!(f, "The manifest must be named project.json"),
			Self::InvalidManifest => write!(f, "The manifest must be a regular file"),
			Self::ManifestIsSymlink => write!(f, "The manifest must not be a symbolic link"),
			Self::MissingRootDirectory => write!(f, "The project root directory is missing"),
			Self::MissingAssetsDirectory => write!(f, "The project assets directory is missing"),
			Self::AssetsNotDirectory => write!(f, "Project assets must be a directory"),
			Self::AssetsNotInProjectDirectory => {
				write!(f, "Project assets must remain inside the project directory")
			}
			Self::Io(error) => write!(f, "Project filesystem error: {error}"),
		}
	}
}

impl std::error::Error for ProjectOpenError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::Io(error) => Some(error),
			_ => None,
		}
	}
}

pub struct ProjectLocation {
	root: PathBuf,
	manifest: PathBuf,
	assets: PathBuf,
}

impl ProjectLocation {
	pub fn from_manifest(path: &Path) -> Result<Self, ProjectOpenError> {
		Self::resolve(path).map_err(|error| {
			engine_logger::error!(
				target: "editor",
				"Operation project_location_open failed: {error}"
			);
			error
		})
	}

	fn resolve(path: &Path) -> Result<Self, ProjectOpenError> {
		if path.file_name().and_then(|name| name.to_str()) != Some("project.json") {
			return Err(ProjectOpenError::InvalidManifestName);
		}

		let metadata = fs::symlink_metadata(path).map_err(ProjectOpenError::Io)?;

		if metadata.file_type().is_symlink() {
			return Err(ProjectOpenError::ManifestIsSymlink);
		}

		if !metadata.is_file() {
			return Err(ProjectOpenError::InvalidManifest);
		}

		let manifest = path.canonicalize().map_err(ProjectOpenError::Io)?;

		let root = manifest
			.parent()
			.ok_or(ProjectOpenError::MissingRootDirectory)?
			.to_path_buf();

		let assets = root.join("assets").canonicalize().map_err(|error| {
			if error.kind() == std::io::ErrorKind::NotFound {
				ProjectOpenError::MissingAssetsDirectory
			} else {
				ProjectOpenError::Io(error)
			}
		})?;

		let assets_metadata = fs::metadata(&assets).map_err(ProjectOpenError::Io)?;

		if !assets_metadata.is_dir() {
			return Err(ProjectOpenError::AssetsNotDirectory);
		}

		if !assets.starts_with(&root) {
			return Err(ProjectOpenError::AssetsNotInProjectDirectory);
		}

		Ok(Self {
			root,
			manifest,
			assets,
		})
	}

	pub fn root(&self) -> &Path {
		&self.root
	}

	pub fn manifest(&self) -> &Path {
		&self.manifest
	}

	pub fn assets(&self) -> &Path {
		&self.assets
	}
}
