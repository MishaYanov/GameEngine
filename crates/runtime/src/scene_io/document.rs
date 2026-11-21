use std::{
	fs::{self, File, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
};

use crate::scene_data::{MAX_SCENE_BYTES, ParsedScene, SceneData, parse_json};

/// Read and validate without constructing GPU objects, for Tauri document commands.
pub fn load_data(path: &Path) -> Result<ParsedScene, String> {
	let file = File::open(path)
		.map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	let mut source = String::new();
	file.take(MAX_SCENE_BYTES as u64 + 1)
		.read_to_string(&mut source)
		.map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	parse_json(&source)
		.map_err(|error| format!("Failed to load scene '{}': {error}", path.display()))
}

/// Atomic persistence shared by the CPU editor and live runtime scene wrapper.
pub fn save_data(path: &Path, data: &SceneData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	save_json(path, &json)
}

/// The development harness persists authored configuration beside its assets.
/// Scene and project files are separate atomic replacements, not a two-file transaction.
pub fn save_project_data(path: &Path, data: &crate::project::ProjectData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	save_json(path, &json)
}

fn save_json(path: &Path, json: &str) -> Result<(), String> {
	let (temporary_path, mut file) = create_temporary_file(path)?;
	let write_result = file
		.write_all(json.as_bytes())
		.and_then(|()| file.sync_all());
	// Close before rename, including on Windows where an open file can block it.
	drop(file);

	if let Err(error) = write_result {
		let _ = fs::remove_file(&temporary_path);
		return Err(format!(
			"Failed to write scene '{}' through '{}': {error}",
			path.display(),
			temporary_path.display()
		));
	}

	// The sibling temporary file is on the same filesystem. Never remove the
	// previous scene first: a failed replacement must leave it available.
	if let Err(error) = fs::rename(&temporary_path, path) {
		let _ = fs::remove_file(&temporary_path);
		return Err(format!(
			"Failed to replace scene '{}': {error}",
			path.display()
		));
	}

	Ok(())
}

fn create_temporary_file(path: &Path) -> Result<(PathBuf, File), String> {
	static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

	let file_name = path
		.file_name()
		.ok_or_else(|| format!("Scene path '{}' has no file name", path.display()))?;

	for _ in 0..128 {
		let sequence = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
		let mut temporary_name = file_name.to_os_string();
		temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
		let temporary_path = path.with_file_name(temporary_name);

		match OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary_path)
		{
			Ok(file) => return Ok((temporary_path, file)),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
			Err(error) => {
				return Err(format!(
					"Failed to create temporary scene file '{}': {error}",
					temporary_path.display()
				));
			}
		}
	}

	Err(format!(
		"Failed to save scene '{}': temporary file names are already in use",
		path.display()
	))
}
