//! Confined paths and bounded reads shared by the bridge asset loaders.
use std::{
	fs,
	io::{ErrorKind, Read},
	path::{Path, PathBuf},
};

use engine_core::scene_data::validate_model_path;

pub(crate) fn contained_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
	optional_contained_file(root, relative)?
		.ok_or_else(|| format!("Asset file '{relative}' is missing"))
}

pub(crate) fn optional_contained_file(
	root: &Path,
	relative: &str,
) -> Result<Option<PathBuf>, String> {
	validate_model_path(relative)?;
	let mut path = root.to_path_buf();
	for component in Path::new(relative).components() {
		path.push(component);
		let metadata = match fs::symlink_metadata(&path) {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(error.to_string()),
		};
		if is_link(&metadata) {
			return Err("Asset paths must not contain symbolic links or reparse points".into());
		}
	}
	let path = path.canonicalize().map_err(|error| error.to_string())?;
	if !path.starts_with(root) {
		return Err("Asset path escapes its asset root".into());
	}
	Ok(Some(path))
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
		if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return true;
		}
	}
	metadata.file_type().is_symlink()
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
	let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
	if is_link(&metadata) || !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Asset file must be a regular file within its byte limit".into());
	}
	let file = fs::File::open(path).map_err(|error| error.to_string())?;
	let metadata = file.metadata().map_err(|error| error.to_string())?;
	if is_link(&metadata) || !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Asset file must be a regular file within its byte limit".into());
	}
	let mut bytes = Vec::new();
	file.take(limit as u64 + 1)
		.read_to_end(&mut bytes)
		.map_err(|error| error.to_string())?;
	if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
		return Err("Asset file changed size while loading".into());
	}
	Ok(bytes)
}
