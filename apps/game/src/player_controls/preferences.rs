use super::{
	ControlsSettings, Preset,
	action_keys::{ActionKeyOverride, validate_rows},
	audio::FeedbackMix,
	display::{DisplaySettings, TextSize},
};
use crate::local_player_files::{lock, ordinary_file, profile_directory, read_bounded};
use runtime::{
	asset_catalog::AssetId,
	input::{ControllerBindings, InputActionLayout, MAX_INPUT_ACTION_MAP_BYTES},
	scene_io::replace_bytes,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::{
	fs,
	sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: usize = 4 * 1024;
const SCHEMA_VERSION: u32 = 6;

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
	display: LegacyDisplay,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDisplay {
	text_size: TextSize,
	high_contrast: bool,
	cue_captions: bool,
}

impl From<LegacyDisplay> for DisplaySettings {
	fn from(display: LegacyDisplay) -> Self {
		Self {
			text_size: display.text_size,
			high_contrast: display.high_contrast,
			cue_captions: display.cue_captions,
			..Self::default()
		}
	}
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordWithAudio<T, D> {
	schema_version: u32,
	movement: Preset,
	controller: ControllerBindings,
	display: D,
	actions: Vec<ActionKeyOverride>,
	audio: T,
}

impl<T: Into<FeedbackMix>, D: Into<DisplaySettings>> RecordWithAudio<T, D> {
	fn into_settings(self) -> Result<ControlsSettings, String> {
		validate_rows(&self.actions)?;
		Ok(ControlsSettings {
			movement: self.movement,
			controller: self.controller.validate()?,
			display: self.display.into(),
			actions: self.actions,
			audio: self.audio.into().validate()?,
		})
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyAudio {
	level_percent: u8,
	muted: bool,
}

impl From<LegacyAudio> for FeedbackMix {
	fn from(audio: LegacyAudio) -> Self {
		Self {
			level_percent: audio.level_percent,
			muted: audio.muted,
			..Self::default()
		}
	}
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Record {
	Sixth(RecordWithAudio<FeedbackMix, DisplaySettings>),
	Fifth(RecordWithAudio<FeedbackMix, LegacyDisplay>),
	Fourth(RecordWithAudio<LegacyAudio, LegacyDisplay>),
	Third(RecordV3),
	Second(RecordV2),
	First(RecordV1),
}

impl Record {
	fn read(bytes: &[u8]) -> Result<ControlsSettings, String> {
		match serde_json::from_slice(bytes).map_err(|_| "Saved controls are invalid")? {
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
				display: record.display.into(),
				..ControlsSettings::default()
			}),
			Self::Fourth(record) if record.schema_version == 4 => record.into_settings(),
			Self::Fifth(record) if record.schema_version == 5 => record.into_settings(),
			Self::Sixth(record) if record.schema_version == SCHEMA_VERSION => {
				record.into_settings()
			}
			_ => Err("Saved controls version is unsupported".into()),
		}
	}
}

#[derive(Clone)]
pub(super) struct Preferences {
	root: PathBuf,
	filename: String,
}

impl Preferences {
	pub(super) fn for_project(project: &Path) -> Result<Self, String> {
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
		// The canonical installation path scopes settings without appearing in the filename.
		let key = Sha256::digest(project.as_os_str().as_encoded_bytes());
		Ok(Self {
			root,
			filename: format!("{key:x}.json"),
		})
	}

	#[cfg(test)]
	pub(super) fn test_profile(root: &Path) -> Self {
		Self {
			root: root.to_owned(),
			filename: "test-controls.json".into(),
		}
	}

	pub(super) fn profile_root(&self) -> &Path {
		&self.root
	}

	/// Bind a review to both canonical preference contents and this local owner.
	pub(super) fn review_digest(&self, controls: &ControlsSettings) -> Result<[u8; 32], String> {
		self.scoped_digest(
			b"game-engine-input-preset-destination-v1",
			&Self::encode(controls)?,
		)
	}

	pub(super) fn library_review_digest(&self, record: &[u8]) -> Result<[u8; 32], String> {
		self.scoped_digest(b"game-engine-input-library-destination-v1", record)
	}

	fn scoped_digest(&self, domain: &[u8], record: &[u8]) -> Result<[u8; 32], String> {
		let profile = self
			.root
			.canonicalize()
			.map_err(|_| "Cannot identify the local profile")?;
		let mut hasher = Sha256::new();
		hasher.update(domain);
		for field in [
			profile.as_os_str().as_encoded_bytes(),
			self.filename.as_bytes(),
		] {
			hasher.update((field.len() as u64).to_le_bytes());
			hasher.update(field);
		}
		hasher.update(record);
		Ok(hasher.finalize().into())
	}

	pub(super) fn load(&self) -> Result<ControlsSettings, String> {
		self.folder(false)?.map_or_else(
			|| Ok(ControlsSettings::default()),
			|folder| self.load_from(&folder),
		)
	}

	pub(super) fn load_action_layout(
		&self,
		asset: &AssetId,
	) -> Result<Option<InputActionLayout>, String> {
		let Some(folder) = self.folder(false)? else {
			return Ok(None);
		};
		Self::read_action_layout(&self.action_layout_path(&folder, asset))
	}

	pub(super) fn save_action_layout(
		&self,
		asset: &AssetId,
		expected: Option<&InputActionLayout>,
		layout: Option<&InputActionLayout>,
	) -> Result<(), String> {
		self.with_record_lock(true, |folder, _| {
			let path = self.action_layout_path(folder, asset);
			if Self::read_action_layout(&path)?.as_ref() != expected {
				return Err(
					"Local keyboard bindings changed. Reopen Keyboard before editing.".into(),
				);
			}
			let bytes = serde_json::to_vec(&layout).map_err(|error| error.to_string())?;
			if bytes.len() > MAX_INPUT_ACTION_MAP_BYTES {
				return Err("Saved Input Actions exceed the local profile limit".into());
			}
			ordinary_file(&path)?;
			replace_bytes(&path, &bytes)
		})
	}

	fn action_layout_path(&self, folder: &Path, asset: &AssetId) -> PathBuf {
		folder
			.join(&self.filename)
			.with_extension(format!("{}.input-actions.json", asset.as_str()))
	}

	fn read_action_layout(path: &Path) -> Result<Option<InputActionLayout>, String> {
		match read_bounded(path, MAX_INPUT_ACTION_MAP_BYTES)? {
			Some(bytes) => {
				serde_json::from_slice(&bytes).map_err(|_| "Saved Input Actions are invalid".into())
			}
			None => Ok(None),
		}
	}

	fn load_from(&self, folder: &Path) -> Result<ControlsSettings, String> {
		read_bounded(&folder.join(&self.filename), MAX_BYTES)?.map_or_else(
			|| Ok(ControlsSettings::default()),
			|bytes| Record::read(&bytes),
		)
	}

	#[cfg(test)]
	pub(super) fn save(&self, controls: &ControlsSettings) -> Result<(), String> {
		self.update(|_| Ok((controls.clone(), ())))
	}

	pub(super) fn save_if_current(
		&self,
		previous: &ControlsSettings,
		controls: &ControlsSettings,
	) -> Result<(), String> {
		self.update(|current| {
			if Self::encode(&current)? != Self::encode(previous)? {
				return Err("Local controls changed. Reopen the Player before editing.".into());
			}
			Ok((controls.clone(), ()))
		})
	}

	pub(super) fn update<T>(
		&self,
		change: impl FnOnce(ControlsSettings) -> Result<(ControlsSettings, T), String>,
	) -> Result<T, String> {
		self.update_record(true, change)
	}

	pub(super) fn update_existing<T>(
		&self,
		change: impl FnOnce(ControlsSettings) -> Result<(ControlsSettings, T), String>,
	) -> Result<T, String> {
		self.update_record(false, change)
	}

	fn update_record<T>(
		&self,
		create: bool,
		change: impl FnOnce(ControlsSettings) -> Result<(ControlsSettings, T), String>,
	) -> Result<T, String> {
		self.with_record_lock(create, |folder, path| {
			let (controls, result) = change(self.load_from(folder)?)?;
			ordinary_file(path)?;
			replace_bytes(path, &Self::encode(&controls)?)?;
			Ok(result)
		})
	}

	pub(super) fn with_record_lock<T>(
		&self,
		create: bool,
		operation: impl FnOnce(&Path, &Path) -> Result<T, String>,
	) -> Result<T, String> {
		let folder = self
			.folder(create)?
			.ok_or("Local controls storage is unavailable")?;
		let path = folder.join(&self.filename);
		let _lock = lock(&path.with_extension("lock"))?;
		operation(&folder, &path)
	}

	pub(super) fn preset_library_path(&self) -> Result<Option<PathBuf>, String> {
		Ok(self.folder(false)?.map(|folder| {
			folder
				.join(&self.filename)
				.with_extension("preset-library.json")
		}))
	}

	fn encode(controls: &ControlsSettings) -> Result<Vec<u8>, String> {
		validate_rows(&controls.actions)?;
		let mut actions = controls.actions.clone();
		actions.sort_unstable_by(|a, b| a.action_id.cmp(&b.action_id));
		let bytes = serde_json::to_vec(&RecordWithAudio {
			schema_version: SCHEMA_VERSION,
			movement: controls.movement,
			controller: controls.controller.validate()?,
			display: controls.display,
			actions,
			audio: controls.audio.validate()?,
		})
		.map_err(|error| format!("Cannot encode controls: {error}"))?;
		if bytes.len() > MAX_BYTES {
			return Err(format!("Saved controls exceed {MAX_BYTES} bytes"));
		}
		Ok(bytes)
	}

	fn folder(&self, create: bool) -> Result<Option<PathBuf>, String> {
		profile_directory(&self.root, &["GameEngine", "controls"], create)
	}
}

#[cfg(test)]
#[path = "../../tests/unit/controls_preferences.rs"]
mod tests;
