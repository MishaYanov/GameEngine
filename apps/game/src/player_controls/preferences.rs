use super::{
	ControlsSettings, Preset,
	action_keys::{ActionKeyOverride, validate_rows},
	audio::FeedbackMix,
	display::DisplaySettings,
};
use runtime::input::ControllerBindings;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
	fs::{self, File, Metadata, OpenOptions},
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: usize = 4 * 1024;
const SCHEMA_VERSION: u32 = 4;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordV1 {
	schema_version: u32,
	movement: Preset,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordV2 {
	schema_version: u32,
	movement: Preset,
	controller: ControllerBindings,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordV3 {
	schema_version: u32,
	movement: Preset,
	controller: ControllerBindings,
	display: DisplaySettings,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordV4 {
	schema_version: u32,
	movement: Preset,
	controller: ControllerBindings,
	display: DisplaySettings,
	actions: Vec<ActionKeyOverride>,
	audio: FeedbackMix,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Record {
	Fourth(RecordV4),
	Third(RecordV3),
	Second(RecordV2),
	First(RecordV1),
}

impl Record {
	fn read(file: File) -> Result<ControlsSettings, String> {
		let mut bytes = Vec::new();
		file.take((MAX_BYTES + 1) as u64)
			.read_to_end(&mut bytes)
			.map_err(|error| format!("Cannot read controls: {error}"))?;
		if bytes.len() > MAX_BYTES {
			return Err(format!("Saved controls exceed {MAX_BYTES} bytes"));
		}
		match serde_json::from_slice(&bytes).map_err(|_| "Saved controls are invalid")? {
			Self::First(record) if record.schema_version == 1 => Ok(ControlsSettings {
				movement: record.movement,
				..ControlsSettings::default()
			}),
			Self::Second(record) if record.schema_version == 2 => Ok(ControlsSettings {
				movement: record.movement,
				controller: record.controller.validate()?,
				..ControlsSettings::default()
			}),
			Self::Third(record) if record.schema_version == 3 => Ok(ControlsSettings {
				movement: record.movement,
				controller: record.controller.validate()?,
				display: record.display,
				..ControlsSettings::default()
			}),
			Self::Fourth(record) if record.schema_version == SCHEMA_VERSION => {
				validate_rows(&record.actions)?;
				Ok(ControlsSettings {
					movement: record.movement,
					controller: record.controller.validate()?,
					display: record.display,
					actions: record.actions,
					audio: record.audio.validate()?,
				})
			}
			_ => Err("Saved controls version is unsupported".into()),
		}
	}
}

pub(super) struct Preferences {
	root: PathBuf,
	filename: String,
}

impl Preferences {
	pub(super) fn for_project(project: &Path, save_id: Option<&str>) -> Result<Self, String> {
		let root = if cfg!(windows) {
			std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
		} else {
			std::env::var_os("XDG_STATE_HOME")
				.map(PathBuf::from)
				.or_else(|| {
					std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
				})
		};
		let root = root
			.filter(|path| path.is_absolute())
			.ok_or("Local controls storage is unavailable")?;
		let project = project
			.canonicalize()
			.map_err(|_| "Cannot identify the local project")?;
		// The installation path scopes settings for projects without an authored
		// save identity. The hash keeps that path out of the filename.
		let key = match save_id {
			Some(id) => Sha256::digest(id.as_bytes()),
			None => Sha256::digest(project.as_os_str().as_encoded_bytes()),
		};
		Ok(Self {
			root,
			filename: format!("{key:x}.json"),
		})
	}

	pub(super) fn load(&self) -> Result<ControlsSettings, String> {
		let Some(folder) = self.folder(false)? else {
			return Ok(ControlsSettings::default());
		};
		let path = folder.join(&self.filename);
		if !regular_record(&path)? {
			return Ok(ControlsSettings::default());
		}
		let mut options = OpenOptions::new();
		options.read(true);
		#[cfg(windows)]
		{
			use std::os::windows::fs::OpenOptionsExt;
			const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
			options.custom_flags(OPEN_REPARSE_POINT);
		}
		let file = options
			.open(path)
			.map_err(|error| format!("Cannot open controls: {error}"))?;
		let metadata = file
			.metadata()
			.map_err(|error| format!("Cannot inspect controls: {error}"))?;
		if !metadata.is_file() || linked(&metadata) {
			return Err("Controls record is not an ordinary file".into());
		}
		Record::read(file)
	}

	pub(super) fn save(&self, controls: &ControlsSettings) -> Result<(), String> {
		validate_rows(&controls.actions)?;
		let bytes = serde_json::to_vec(&RecordV4 {
			schema_version: SCHEMA_VERSION,
			movement: controls.movement,
			controller: controls.controller.validate()?,
			display: controls.display,
			actions: controls.actions.clone(),
			audio: controls.audio.validate()?,
		})
		.map_err(|error| format!("Cannot encode controls: {error}"))?;
		if bytes.len() > MAX_BYTES {
			return Err(format!("Saved controls exceed {MAX_BYTES} bytes"));
		}
		let folder = self
			.folder(true)?
			.ok_or("Local controls storage is unavailable")?;
		let path = folder.join(&self.filename);
		regular_record(&path)?;
		let (temporary, mut file) = temporary_sibling(&path)
			.map_err(|error| format!("Cannot prepare controls: {error}"))?;
		let written = file.write_all(&bytes).and_then(|()| file.sync_all());
		drop(file);
		let result = written.and_then(|()| fs::rename(&temporary, path));
		if let Err(error) = result {
			let _ = fs::remove_file(temporary);
			return Err(format!("Cannot save controls: {error}"));
		}
		Ok(())
	}

	fn folder(&self, create: bool) -> Result<Option<PathBuf>, String> {
		let root = self
			.root
			.canonicalize()
			.map_err(|_| "Local controls storage is unavailable")?;
		let mut path = root;
		for segment in ["GameEngine", "controls"] {
			path.push(segment);
			if create {
				match fs::create_dir(&path) {
					Ok(()) => {}
					Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
					Err(error) => return Err(format!("Cannot create controls folder: {error}")),
				}
			}
			match fs::symlink_metadata(&path) {
				Ok(metadata) if metadata.is_dir() && !linked(&metadata) => {}
				Ok(_) => return Err("Controls folder is not an ordinary directory".into()),
				Err(error) if error.kind() == io::ErrorKind::NotFound && !create => {
					return Ok(None);
				}
				Err(error) => return Err(format!("Cannot inspect controls folder: {error}")),
			}
		}
		Ok(Some(path))
	}
}

fn regular_record(path: &Path) -> Result<bool, String> {
	match fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_file() && !linked(&metadata) => Ok(true),
		Ok(_) => Err("Controls record is not an ordinary file".into()),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
		Err(error) => Err(format!("Cannot inspect saved controls: {error}")),
	}
}

fn linked(metadata: &Metadata) -> bool {
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

fn temporary_sibling(path: &Path) -> io::Result<(PathBuf, File)> {
	static NEXT: AtomicU64 = AtomicU64::new(0);
	for _ in 0..128 {
		let id = NEXT.fetch_add(1, Ordering::Relaxed);
		let temporary = path.with_extension(format!("tmp-{}-{id}", std::process::id()));
		match OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary)
		{
			Ok(file) => return Ok((temporary, file)),
			Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
			Err(error) => return Err(error),
		}
	}
	Err(io::Error::new(
		io::ErrorKind::AlreadyExists,
		"Controls temporary file names exhausted",
	))
}

#[cfg(test)]
#[path = "../../tests/unit/controls_preferences.rs"]
mod tests;
