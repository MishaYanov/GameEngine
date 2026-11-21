//! Confined paths and bounded reads shared by the bridge asset loaders.
use std::{fs, io::Read, path::Path};

use engine_core::scene_data::validate_model_path;

pub(crate) fn contained_file(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
	validate_model_path(relative)?;
	let mut path = root.to_path_buf();
	for component in Path::new(relative).components() {
		path.push(component);
		if fs::symlink_metadata(&path)
			.map_err(|error| error.to_string())?
			.file_type()
			.is_symlink()
		{
			return Err("Runtime prefab paths must not contain symbolic links".into());
		}
	}
	let path = path.canonicalize().map_err(|error| error.to_string())?;
	if !path.starts_with(root) {
		return Err("Runtime prefab escapes its asset root".into());
	}
	Ok(path)
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
	let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
	if !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Runtime prefab/catalog must be a regular file within its byte limit".into());
	}
	let file = fs::File::open(path).map_err(|error| error.to_string())?;
	let metadata = file.metadata().map_err(|error| error.to_string())?;
	if !metadata.is_file() || metadata.len() > limit as u64 {
		return Err("Runtime prefab/catalog must be a regular file within its byte limit".into());
	}
	let mut bytes = Vec::new();
	file.take(limit as u64 + 1)
		.read_to_end(&mut bytes)
		.map_err(|error| error.to_string())?;
	if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
		return Err("Runtime prefab/catalog changed size while loading".into());
	}
	Ok(bytes)
}
