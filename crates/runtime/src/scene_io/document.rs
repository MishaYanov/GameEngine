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
	load_opened_data(path, file)
}

/// Load a project scene through the verified file handle and report its canonical path.
pub fn load_confined_data(root: &Path, relative: &Path) -> Result<(PathBuf, ParsedScene), String> {
	let root = root.canonicalize().map_err(|error| error.to_string())?;
	load_confined_data_from_canonical_root(&root, relative)
}

/// Load against a previously captured canonical root without resolving it again.
pub fn load_confined_data_from_canonical_root(
	root: &Path,
	relative: &Path,
) -> Result<(PathBuf, ParsedScene), String> {
	let (path, source) = read_confined_scene_source(root, relative)?;
	let parsed = parse_source(&path, &source)?;
	Ok((path, parsed))
}

/// Read bounded UTF-8 for callers that also need the original bytes, such as checkpoint hashing.
/// The caller supplies its captured canonical asset root.
pub fn read_confined_scene_source(
	root: &Path,
	relative: &Path,
) -> Result<(PathBuf, String), String> {
	let (path, file) = super::paths::open_confined_scene(root, relative)?;
	let source = read_opened_source(&path, file)?;
	Ok((path, source))
}

pub(crate) fn load_opened_data(path: &Path, file: File) -> Result<ParsedScene, String> {
	parse_source(path, &read_opened_source(path, file)?)
}

fn read_opened_source(path: &Path, file: File) -> Result<String, String> {
	let mut source = String::new();
	file.take(MAX_SCENE_BYTES as u64 + 1)
		.read_to_string(&mut source)
		.map_err(|error| format!("Failed to read scene '{}': {error}", path.display()))?;
	if source.len() > MAX_SCENE_BYTES {
		return Err(format!(
			"Scene '{}' exceeds {MAX_SCENE_BYTES} bytes",
			path.display()
		));
	}
	Ok(source)
}

fn parse_source(path: &Path, source: &str) -> Result<ParsedScene, String> {
	parse_json(source)
		.map_err(|error| format!("Failed to load scene '{}': {error}", path.display()))
}

/// Atomic persistence shared by the CPU editor and live runtime scene wrapper.
pub fn save_data(path: &Path, data: &SceneData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	replace_bytes(path, json.as_bytes())
}

/// The development harness persists authored configuration beside its assets.
/// Scene and project files are separate atomic replacements, not a two-file transaction.
pub fn save_project_data(path: &Path, data: &crate::project::ProjectData) -> Result<(), String> {
	let mut json = data.to_json()?;
	json.push('\n');
	replace_bytes(path, json.as_bytes())
}

/// Publish a flushed sibling file without replacing an occupied destination.
pub fn save_new_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
	let (temporary, mut file) = create_temporary_file(path)?;
	let flushed = file.write_all(bytes).and_then(|()| file.sync_all());
	drop(file);
	let result = flushed
		.map_err(|error| error.to_string())
		.and_then(|()| crate::assets::project_assets::install_new(&temporary, path));
	let _ = fs::remove_file(temporary);
	result
}

/// Atomically replace an authored file with a flushed sibling. The caller owns
/// path checks and any expected-content comparison before publication.
pub fn replace_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
	let (temporary_path, mut file) = create_temporary_file(path)?;
	let write_result = file.write_all(bytes).and_then(|()| file.sync_all());
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
