//! Ordinary authored scene resolution before obtaining a CPU or GPU document.
use std::{
	fs,
	path::{Component, Path, PathBuf},
};

pub fn resolve_confined_scene_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	resolve_from_canonical_root(&root, relative)
}

pub(crate) fn resolve_from_canonical_root(root: &Path, relative: &Path) -> Result<PathBuf, String> {
	let mut path = root.to_path_buf();
	for component in relative.components() {
		let Component::Normal(name) = component else {
			return Err("Scene path must be a relative asset path".into());
		};
		path.push(name);
		let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
		if is_linked(&metadata) {
			return Err("Scene path includes a linked package entry".into());
		}
	}
	let scene = path.canonicalize().map_err(|error| error.to_string())?;
	if !scene.starts_with(root) || !scene.is_file() {
		return Err("Scene must resolve to a regular file inside the asset root".into());
	}
	Ok(scene)
}

pub(crate) fn is_linked(metadata: &fs::Metadata) -> bool {
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		const REPARSE_POINT: u32 = 0x400;
		if metadata.file_attributes() & REPARSE_POINT != 0 {
			return true;
		}
	}
	metadata.file_type().is_symlink()
}
