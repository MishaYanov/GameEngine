//! Ordinary, bounded files used by local Player settings and transfers.

use crate::play::is_linked_package_entry;
use std::{
	fs::{self, File, OpenOptions, TryLockError},
	io::{self, Read},
	path::{Path, PathBuf},
	time::Duration,
};

pub(crate) fn ordinary_file(path: &Path) -> Result<bool, String> {
	match fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_file() && !is_linked_package_entry(&metadata) => Ok(true),
		Ok(_) => Err("Expected an ordinary local file".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
		Err(error) => Err(error.to_string()),
	}
}

pub(crate) fn ordinary_open(path: &Path, options: &mut OpenOptions) -> Result<File, String> {
	#[cfg(windows)]
	{
		use std::os::windows::fs::OpenOptionsExt;
		const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
		options.custom_flags(OPEN_REPARSE_POINT);
	}
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
	}
	let file = options.open(path).map_err(|error| error.to_string())?;
	let metadata = file.metadata().map_err(|error| error.to_string())?;
	if !metadata.is_file() || is_linked_package_entry(&metadata) || !ordinary_file(path)? {
		return Err("Expected an ordinary local file".into());
	}
	Ok(file)
}

pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
	if !ordinary_file(path)? {
		return Ok(None);
	}
	let file = ordinary_open(path, OpenOptions::new().read(true))?;
	let mut bytes = Vec::new();
	file.take((limit + 1) as u64)
		.read_to_end(&mut bytes)
		.map_err(|error| error.to_string())?;
	if bytes.len() > limit {
		return Err(format!("Local file exceeds {limit} bytes"));
	}
	Ok(Some(bytes))
}

/// Cooperating processes keep this handle alive through publication.
pub(crate) fn lock(path: &Path) -> Result<File, String> {
	ordinary_file(path)?;
	let file = ordinary_open(
		path,
		OpenOptions::new()
			.read(true)
			.write(true)
			.create(true)
			.truncate(false),
	)?;
	for _ in 0..20 {
		match file.try_lock() {
			Ok(()) => return Ok(file),
			Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
			Err(error) => return Err(format!("Cannot lock the local record: {error}")),
		}
	}
	Err("The local record is busy. Try again.".into())
}

pub(crate) fn ordinary_directory(
	path: &Path,
	create: bool,
	optional: bool,
) -> Result<Option<PathBuf>, String> {
	if create {
		match fs::create_dir(path) {
			Ok(()) => {}
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
			Err(error) => return Err(error.to_string()),
		}
	}
	match fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_dir() && !is_linked_package_entry(&metadata) => path
			.canonicalize()
			.map(Some)
			.map_err(|error| error.to_string()),
		Ok(_) => Err("Expected an ordinary local directory".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound && optional => Ok(None),
		Err(error) => Err(error.to_string()),
	}
}

pub(crate) fn profile_directory(
	root: &Path,
	segments: &[&str],
	create: bool,
) -> Result<Option<PathBuf>, String> {
	let mut folder = root.canonicalize().map_err(|error| error.to_string())?;
	for segment in segments {
		let Some(next) = ordinary_directory(&folder.join(segment), create, true)? else {
			return Ok(None);
		};
		folder = next;
	}
	Ok(Some(folder))
}

const MAX_TRANSFER_FILES: usize = 32;
const MAX_TRANSFER_ENTRIES: usize = 512;

pub(crate) fn ordinary_transfer(path: &Path, suffix: &str) -> Result<Option<String>, String> {
	let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
		return Ok(None);
	};
	if !name.ends_with(suffix) || name.len() > 96 || name.chars().any(char::is_control) {
		return Ok(None);
	}
	match fs::symlink_metadata(path) {
		Ok(metadata) => {
			Ok((metadata.is_file() && !is_linked_package_entry(&metadata)).then(|| name.into()))
		}
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.to_string()),
	}
}

pub(crate) fn transfer_entries(
	directory: &Path,
	suffix: &str,
) -> Result<Vec<(String, PathBuf)>, String> {
	let mut files = Vec::new();
	for (index, entry) in fs::read_dir(directory)
		.map_err(|error| error.to_string())?
		.enumerate()
	{
		if index == MAX_TRANSFER_ENTRIES {
			return Err(
				"Transfer folder has more than 512 entries. Choose a smaller folder.".into(),
			);
		}
		let path = entry.map_err(|error| error.to_string())?.path();
		if let Some(name) = ordinary_transfer(&path, suffix)? {
			files.push((name, path));
		}
	}
	files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
	files.truncate(MAX_TRANSFER_FILES);
	Ok(files)
}

pub(crate) fn transfer_label(noun: &str, number: usize, name: &str, suffix: &str) -> String {
	// Keep the index visible even when the bitmap font clips or substitutes a name.
	let mut basename = name.strip_suffix(suffix).unwrap_or(name).chars();
	let visible: String = basename
		.by_ref()
		.take(22)
		.map(|character| if character.is_ascii() { character } else { '?' })
		.collect();
	let continuation = if basename.next().is_some() { "..." } else { "" };
	format!("{noun} {number:02}: {visible}{continuation}")
}
