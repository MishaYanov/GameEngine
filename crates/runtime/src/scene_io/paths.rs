//! Ordinary authored scene resolution before obtaining a CPU or GPU document.
use std::{
	fs::{self, File},
	io,
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

/// Open and verify one file before any JSON is read from it.
pub(crate) fn open_confined_scene(root: &Path, relative: &Path) -> Result<(PathBuf, File), String> {
	let path = resolve_from_canonical_root(root, relative)?;
	let file = open_verified(root, relative, &path)
		.map_err(|error| format!("Failed to open scene '{}': {error}", path.display()))?;
	let metadata = file.metadata().map_err(|error| error.to_string())?;
	if !metadata.is_file() || is_linked(&metadata) {
		return Err("Scene source must be an ordinary file".into());
	}
	Ok((path, file))
}

#[cfg(windows)]
fn open_verified(root: &Path, _relative: &Path, path: &Path) -> io::Result<File> {
	use std::os::windows::fs::OpenOptionsExt;
	use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

	let file = fs::OpenOptions::new()
		.read(true)
		.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
		.open(path)?;
	verify_scene_handle(root, &file)?;
	Ok(file)
}

#[cfg(windows)]
fn verify_scene_handle(root: &Path, file: &File) -> io::Result<()> {
	use std::{
		ffi::OsString,
		os::windows::{ffi::OsStringExt, io::AsRawHandle},
	};
	use windows_sys::Win32::Storage::FileSystem::{
		FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
	};

	let mut name = vec![0u16; 32_768];
	// SAFETY: the file stays open and name supplies the advertised writable capacity.
	let length = unsafe {
		GetFinalPathNameByHandleW(
			file.as_raw_handle() as _,
			name.as_mut_ptr(),
			name.len() as u32,
			FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
		)
	} as usize;
	if length == 0 {
		return Err(io::Error::last_os_error());
	}
	if length >= name.len() {
		return Err(io::Error::new(
			io::ErrorKind::InvalidData,
			"Scene path is too long",
		));
	}
	let actual = PathBuf::from(OsString::from_wide(&name[..length]));
	if actual == root || !actual.starts_with(root) {
		return Err(io::Error::new(
			io::ErrorKind::PermissionDenied,
			"Opened scene file is outside the asset root",
		));
	}
	Ok(())
}

#[cfg(unix)]
fn open_verified(root: &Path, relative: &Path, _path: &Path) -> io::Result<File> {
	use std::{
		ffi::CString,
		os::{
			fd::{AsRawFd, FromRawFd},
			unix::{ffi::OsStrExt, fs::OpenOptionsExt},
		},
	};

	let directory_flags = directory_search_flags() | libc::O_NOFOLLOW | libc::O_CLOEXEC;
	let mut directory = fs::OpenOptions::new()
		.read(true)
		.custom_flags(directory_flags)
		.open("/")?;
	let absolute = root.join(relative);
	let mut components = absolute.components();
	if components.next() != Some(Component::RootDir) {
		return Err(io::Error::new(
			io::ErrorKind::InvalidInput,
			"Expected an absolute scene path",
		));
	}
	let mut components = components.peekable();
	while let Some(Component::Normal(name)) = components.next() {
		let name = CString::new(name.as_bytes())?;
		let last = components.peek().is_none();
		let flags = if last {
			libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK
		} else {
			directory_flags
		};
		// SAFETY: directory is open, name is NUL-terminated, and these flags do not create a file.
		let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
		if descriptor < 0 {
			return Err(io::Error::last_os_error());
		}
		// SAFETY: openat returned a fresh owned descriptor.
		let opened = unsafe { File::from_raw_fd(descriptor) };
		if last {
			return Ok(opened);
		}
		directory = opened;
	}
	Err(io::Error::new(
		io::ErrorKind::InvalidInput,
		"Expected an ordinary scene path",
	))
}

#[cfg(unix)]
fn directory_search_flags() -> libc::c_int {
	// These descriptor modes require search permission without directory listing access.
	#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
	return libc::O_DIRECTORY | libc::O_PATH;
	#[cfg(target_vendor = "apple")]
	return libc::O_SEARCH;
	#[cfg(not(any(
		target_os = "linux",
		target_os = "android",
		target_os = "freebsd",
		target_vendor = "apple",
	)))]
	return libc::O_DIRECTORY;
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

#[cfg(all(test, windows))]
mod tests {
	use super::*;

	#[test]
	fn the_open_handle_must_resolve_inside_the_asset_root() {
		let source = Path::new(env!("CARGO_MANIFEST_DIR"))
			.canonicalize()
			.unwrap();
		let file = File::open(source.join("Cargo.toml")).unwrap();
		assert!(verify_scene_handle(&source, &file).is_ok());
		let error = verify_scene_handle(&source.join("src"), &file).unwrap_err();
		assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
	}
}
