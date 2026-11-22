//! Portable input choices and revision-bound merges into local controls.

use super::{
	ControlsSettings, Preset,
	action_keys::ActionKeyOverride,
	preferences::Preferences,
	preset_library::{LibraryBackupReview, PresetLibrary, SavedLayout},
};
use crate::local_player_files::{ordinary_directory, profile_directory, read_bounded};
use runtime::{
	Key,
	input::{ControllerBindings, InputProfile},
	project::{MAX_PROJECT_BYTES, ProjectData},
	scene_io::save_new_bytes,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_PRESET_BYTES: usize = 4 * 1024;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
	tag = "kind",
	content = "value",
	rename_all = "snake_case",
	deny_unknown_fields
)]
enum ProjectIdentity {
	ManifestSha256(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InputChoices {
	movement: Preset,
	controller: ControllerBindings,
	actions: Vec<ActionKeyOverride>,
}

impl InputChoices {
	fn from_controls(controls: &ControlsSettings, authored: &InputProfile) -> Result<Self, String> {
		controls.resolve_keyboard(Some(authored))?;
		let mut actions = controls.actions.clone();
		actions.sort_unstable_by_key(|row| {
			authored
				.actions
				.iter()
				.position(|action| action.id == row.action_id)
		});
		Ok(Self {
			movement: controls.movement,
			controller: controls.controller.validate()?,
			actions,
		})
	}

	fn merge(self, local: ControlsSettings) -> Result<ControlsSettings, String> {
		Ok(ControlsSettings {
			movement: self.movement,
			controller: self.controller.validate()?,
			actions: self.actions,
			..local
		})
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortablePreset {
	schema_version: u32,
	project: ProjectIdentity,
	authored_input_sha256: String,
	input: InputChoices,
}

impl PortablePreset {
	pub(super) fn encoded(&self) -> Result<Vec<u8>, String> {
		let data = serde_json::to_vec(self)
			.map_err(|error| format!("Cannot encode input preset: {error}"))?;
		if data.len() > MAX_PRESET_BYTES {
			return Err("Input preset exceeds 4 KiB".into());
		}
		Ok(data)
	}
}

#[derive(Debug, Serialize)]
pub(crate) struct Change<T> {
	pub(super) current: T,
	pub(super) proposed: T,
}

#[derive(Debug, Serialize)]
pub(crate) struct ActionChange {
	pub(super) id: String,
	pub(super) keys: Change<Key>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PresetPreview {
	#[serde(serialize_with = "serialize_digest")]
	pub(crate) source_sha256: [u8; 32],
	#[serde(serialize_with = "serialize_digest")]
	pub(crate) destination_sha256: [u8; 32],
	pub(super) movement: Change<Preset>,
	pub(super) controller: Change<ControllerBindings>,
	pub(super) actions: Vec<ActionChange>,
}

#[derive(PartialEq)]
struct Scope {
	project: ProjectIdentity,
	authored: InputProfile,
	authored_sha256: String,
}

impl Scope {
	fn read(manifest: &Path) -> Result<(Self, ProjectData), String> {
		let bytes = read_bounded(manifest, MAX_PROJECT_BYTES)?
			.ok_or("Input presets require an installed project.json")?;
		let text = std::str::from_utf8(&bytes).map_err(|_| "Project manifest is not UTF-8")?;
		let project = ProjectData::from_json(text)?;
		let project_id =
			ProjectIdentity::ManifestSha256(encode_digest(&Sha256::digest(&bytes).into()));
		let authored = project.input_profile.clone().unwrap_or_default();
		super::validate_profile(&authored)?;
		let encoded = serde_json::to_vec(&authored).map_err(|error| error.to_string())?;
		let authored_sha256 = encode_digest(&Sha256::digest(&encoded).into());
		Ok((
			Self {
				project: project_id,
				authored,
				authored_sha256,
			},
			project,
		))
	}

	fn record(&self, settings: &ControlsSettings) -> Result<PortablePreset, String> {
		Ok(PortablePreset {
			schema_version: SCHEMA_VERSION,
			project: self.project.clone(),
			authored_input_sha256: self.authored_sha256.clone(),
			input: InputChoices::from_controls(settings, &self.authored)?,
		})
	}

	fn proposed(
		&self,
		local: ControlsSettings,
		record: PortablePreset,
	) -> Result<(ControlsSettings, InputProfile), String> {
		if record.schema_version != SCHEMA_VERSION {
			return Err("Input preset uses an unsupported version".into());
		}
		if record.project != self.project {
			return Err("Input preset belongs to a different authored project".into());
		}
		if record.authored_input_sha256 != self.authored_sha256 {
			return Err("Input preset was made for a different authored input profile".into());
		}
		let settings = record.input.merge(local)?;
		let profile = settings.resolve_keyboard(Some(&self.authored))?;
		Ok((settings, profile))
	}
}

pub(crate) struct PresetTransfer {
	manifest: PathBuf,
	scope: Scope,
	preferences: Preferences,
}

impl PresetTransfer {
	pub(crate) fn new(manifest: &Path, running: &ProjectData) -> Result<Self, String> {
		let (scope, current) = Scope::read(manifest)?;
		if running != &current {
			return Err("The installed project changed. Reopen the Player.".into());
		}
		let preferences = Preferences::for_project(manifest)?;
		Ok(Self {
			manifest: manifest.to_owned(),
			scope,
			preferences,
		})
	}

	pub(crate) fn export(&self, path: &Path) -> Result<(), String> {
		self.export_applied(path, &self.preferences.load()?)
	}

	pub(super) fn export_applied(
		&self,
		path: &Path,
		settings: &ControlsSettings,
	) -> Result<(), String> {
		self.verify_scope()?;
		let data = self.scope.record(settings)?.encoded()?;
		let parent = path
			.parent()
			.filter(|parent| !parent.as_os_str().is_empty())
			.unwrap_or(Path::new("."));
		ordinary_directory(parent, false, false)?;
		save_new_bytes(path, &data)
	}

	pub(crate) fn preview(&self, path: &Path) -> Result<PresetPreview, String> {
		self.preview_source(|| read_preset(path))
	}

	pub(super) fn library(&self) -> PresetLibrary<'_> {
		PresetLibrary::new(&self.preferences)
	}

	pub(super) fn export_library(&self, path: &Path) -> Result<usize, String> {
		self.library().export(
			path,
			|| self.verify_scope(),
			|preset| self.validate_library_preset(preset),
		)
	}

	pub(super) fn preview_library_backup(
		&self,
		path: &Path,
	) -> Result<LibraryBackupReview, String> {
		self.verify_scope()?;
		self.library()
			.review_backup(path, |preset| self.validate_library_preset(preset))
	}

	pub(super) fn restore_library_backup(
		&self,
		path: &Path,
		review: &LibraryBackupReview,
	) -> Result<usize, String> {
		self.library().restore_backup(
			path,
			review,
			|| self.verify_scope(),
			|preset| self.validate_library_preset(preset),
		)
	}

	fn validate_library_preset(&self, preset: &PortablePreset) -> Result<(), String> {
		self.scope
			.proposed(ControlsSettings::default(), preset.clone())
			.map(|_| ())
	}

	pub(super) fn save_layout(
		&self,
		label: &str,
		settings: &ControlsSettings,
	) -> Result<(), String> {
		self.verify_scope()?;
		self.library().save(label, self.scope.record(settings)?)
	}

	pub(super) fn preview_layout(&self, layout: &SavedLayout) -> Result<PresetPreview, String> {
		self.preview_source(|| self.library().source(layout))
	}

	pub(super) fn apply_layout(
		&self,
		layout: &SavedLayout,
		review: &PresetPreview,
	) -> Result<(ControlsSettings, InputProfile), String> {
		self.apply_source(
			|| self.library().source(layout),
			review.source_sha256,
			review.destination_sha256,
			true,
		)
	}

	fn preview_source(
		&self,
		read: impl FnOnce() -> Result<Vec<u8>, String>,
	) -> Result<PresetPreview, String> {
		self.verify_scope()?;
		let current = self.preferences.load()?;
		let destination_sha256 = self.preferences.review_digest(&current)?;
		let source = read()?;
		let source_sha256 = Sha256::digest(&source).into();
		let record = decode_preset(&source)?;
		let current_profile = current
			.resolve_keyboard(Some(&self.scope.authored))
			.unwrap_or_else(|_| self.scope.authored.clone());
		let (proposed, profile) = self.scope.proposed(current.clone(), record)?;
		let actions = current_profile
			.actions
			.iter()
			.zip(profile.actions)
			.map(|(current, proposed)| ActionChange {
				id: proposed.id,
				keys: Change {
					current: current.key,
					proposed: proposed.key,
				},
			})
			.collect();
		Ok(PresetPreview {
			source_sha256,
			destination_sha256,
			movement: Change {
				current: current.movement,
				proposed: proposed.movement,
			},
			controller: Change {
				current: current.controller,
				proposed: proposed.controller,
			},
			actions,
		})
	}

	pub(crate) fn apply(
		&self,
		path: &Path,
		source: [u8; 32],
		destination: [u8; 32],
	) -> Result<(), String> {
		self.apply_reviewed(path, source, destination).map(|_| ())
	}

	pub(super) fn apply_reviewed(
		&self,
		path: &Path,
		source: [u8; 32],
		destination: [u8; 32],
	) -> Result<(ControlsSettings, InputProfile), String> {
		self.apply_source(|| read_preset(path), source, destination, false)
	}

	fn apply_source(
		&self,
		read: impl Fn() -> Result<Vec<u8>, String>,
		source: [u8; 32],
		destination: [u8; 32],
		existing_library: bool,
	) -> Result<(ControlsSettings, InputProfile), String> {
		let check = |current: ControlsSettings| {
			self.verify_scope()?;
			if self.preferences.review_digest(&current)? != destination {
				return Err("Local controls changed. Review the input preset again.".into());
			}
			let bytes = read()?;
			if <[u8; 32]>::from(Sha256::digest(&bytes)) != source {
				return Err("Input preset changed. Review it again.".into());
			}
			self.scope.proposed(current, decode_preset(&bytes)?)
		};
		let apply = |current| {
			let (settings, profile) = check(current)?;
			Ok((settings.clone(), (settings, profile)))
		};
		if existing_library {
			self.preferences.update_existing(apply)
		} else {
			// Reject an already stale file review before creating a preferences folder.
			check(self.preferences.load()?)?;
			self.preferences.update(apply)
		}
	}

	pub(super) fn transfer_directory(
		&self,
		configured: Option<&Path>,
		create: bool,
	) -> Result<Option<PathBuf>, String> {
		if let Some(path) = configured {
			return ordinary_directory(path, false, false);
		}
		profile_directory(
			self.preferences.profile_root(),
			&["GameEngine", "Input Presets"],
			create,
		)
	}

	fn verify_scope(&self) -> Result<(), String> {
		if Scope::read(&self.manifest)?.0 != self.scope {
			return Err("The installed input profile changed. Reopen the Player.".into());
		}
		Ok(())
	}
}

fn read_preset(path: &Path) -> Result<Vec<u8>, String> {
	read_bounded(path, MAX_PRESET_BYTES)?.ok_or_else(|| "Input preset file is unavailable".into())
}

fn decode_preset(source: &[u8]) -> Result<PortablePreset, String> {
	serde_json::from_slice(source).map_err(|error| format!("Invalid input preset: {error}"))
}

fn encode_digest(digest: &[u8; 32]) -> String {
	digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn serialize_digest<S: serde::Serializer>(digest: &[u8; 32], output: S) -> Result<S::Ok, S::Error> {
	output.serialize_str(&encode_digest(digest))
}

#[cfg(test)]
#[path = "../../tests/unit/input_preset_transfer.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/preset_library.rs"]
mod library_tests;
