//! Shared checkpoint transfer operations for Player Controls and the offline CLI.

use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	time::{SystemTime, UNIX_EPOCH},
};

use courtyard_gameplay::checkpoint::{
	CheckpointImport, CheckpointRegistry, ImportIntent, ImportPreview, ImportReview, SaveSlot,
};
use game::launch::CheckpointTransfer;
use runtime::{SceneData, project::ProjectData, scene_catalog::SceneId};

use super::{
	GameApp,
	checkpoint_controls::storage_root,
	play::{is_linked_package_entry, resolve_scene_path},
};

const SUFFIX: &str = ".courtyard-save.json";
const MAX_FILES: usize = 32;
const MAX_SCANNED_ENTRIES: usize = 512;

pub(super) struct TransferFile {
	pub label: String,
	pub path: PathBuf,
}

struct TransferOwner<'a> {
	registry: &'a CheckpointRegistry,
	project: &'a ProjectData,
	profile: PathBuf,
	assets: &'a Path,
}

impl TransferOwner<'_> {
	fn authored(&self, scene: &SceneId) -> Result<SceneData, String> {
		let path = resolve_scene_path(self.assets, self.project.scene_path(scene)?)?;
		Ok(runtime::scene_io::load_data(&path)?.data)
	}

	fn export(&self, slot: SaveSlot, path: &Path) -> Result<(), String> {
		let expected = self
			.registry
			.slot_transfer_generation(&self.profile, slot)?;
		let bytes = self.registry.export_slot_transfer(
			&self.profile,
			slot,
			&expected,
			self.project,
			|scene| self.authored(scene),
		)?;
		runtime::scene_io::save_new_bytes(path, &bytes)
	}

	fn preview(&self, slot: SaveSlot, path: &Path) -> Result<ImportPreview, String> {
		self.registry
			.preview_slot_transfer(&self.profile, slot, path, self.project, |scene| {
				self.authored(scene)
			})
	}

	fn import(
		&self,
		slot: SaveSlot,
		archive: &Path,
		review: &ImportReview,
		intent: ImportIntent,
	) -> Result<(), String> {
		self.registry.import_slot_transfer(
			CheckpointImport {
				profile: &self.profile,
				slot,
				archive,
				review,
				intent,
				project: self.project,
			},
			|scene| self.authored(scene),
		)
	}
}

impl GameApp {
	fn transfer_owner(&self) -> Result<TransferOwner<'_>, String> {
		Ok(TransferOwner {
			registry: self
				.checkpoints
				.as_ref()
				.ok_or("Checkpoints are unavailable")?,
			project: self.project.as_ref().ok_or("Project is unavailable")?,
			profile: storage_root()?,
			assets: self
				.asset_root
				.as_deref()
				.ok_or("Project assets are unavailable")?,
		})
	}

	pub(super) fn checkpoint_transfer_folder_label(&self) -> String {
		self.transfer_directory
			.as_ref()
			.and_then(|path| path.file_name())
			.map_or_else(
				|| "Checkpoint Transfers".into(),
				|name| {
					name.to_string_lossy()
						.chars()
						.filter(|ch| !ch.is_control())
						.take(40)
						.collect()
				},
			)
	}

	fn checkpoint_transfer_directory(&self, create: bool) -> Result<Option<PathBuf>, String> {
		if let Some(path) = self.transfer_directory.as_deref() {
			return ordinary_directory(path, false, false);
		}
		let profile = storage_root()?;
		let root = ordinary_directory(&profile, false, false)?
			.ok_or("Checkpoint profile is unavailable")?;
		let Some(game) = ordinary_directory(&root.join("GameEngine"), create, true)? else {
			return Ok(None);
		};
		ordinary_directory(&game.join("Checkpoint Transfers"), create, true)
	}

	pub(super) fn checkpoint_transfer_files(&self) -> Result<Vec<TransferFile>, String> {
		self.checkpoint_transfer_directory(false)?
			.map_or_else(|| Ok(Vec::new()), |folder| files_in(&folder))
	}

	pub(super) fn export_checkpoint_transfer(&self, slot: SaveSlot) -> Result<String, String> {
		let owner = self.transfer_owner()?;
		let seconds = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|error| error.to_string())?
			.as_secs();
		let name = export_name(slot, seconds);
		let directory = self
			.checkpoint_transfer_directory(true)?
			.ok_or("Checkpoint transfer folder is unavailable")?;
		owner.export(slot, &directory.join(&name))?;
		Ok(format!("Exported {name}"))
	}

	pub(super) fn preview_checkpoint_transfer(
		&self,
		slot: SaveSlot,
		file: &TransferFile,
	) -> Result<ImportPreview, String> {
		self.check_transfer_selection(file)?;
		self.transfer_owner()?.preview(slot, &file.path)
	}

	pub(super) fn import_checkpoint_transfer(
		&self,
		slot: SaveSlot,
		file: &TransferFile,
		preview: &ImportPreview,
	) -> Result<String, String> {
		if self.play_trace.is_some() {
			return Err("Checkpoint import is unavailable during playtrace recording".into());
		}
		self.check_transfer_selection(file)?;
		let review = ImportReview::from_hex(
			&preview.archive_sha256,
			&preview.destination_generation_sha256,
		)?;
		let intent = if preview.replacement_required {
			ImportIntent::ReplaceCurrent
		} else {
			ImportIntent::EmptyOnly
		};
		self.transfer_owner()?
			.import(slot, &file.path, &review, intent)?;
		Ok(format!(
			"Imported {}. Select Continue to resume.",
			slot.label()
		))
	}

	fn check_transfer_selection(&self, file: &TransferFile) -> Result<(), String> {
		let directory = self
			.checkpoint_transfer_directory(false)?
			.ok_or("Transfer folder is unavailable")?;
		if file.path.parent() != Some(directory.as_path())
			|| ordinary_transfer(&file.path)?.is_none()
		{
			return Err("Transfer file changed. Reopen the transfer list.".into());
		}
		Ok(())
	}
}

pub(super) fn execute(
	command: CheckpointTransfer,
	project: Option<&ProjectData>,
	install_root: &Path,
) -> Result<(), String> {
	let project = project.ok_or("Checkpoint transfer requires an installed project.json")?;
	let registry = CheckpointRegistry::from_project(project)?
		.ok_or("The installed project has no checkpoint registry")?;
	let assets = install_root.join("assets");
	let owner = TransferOwner {
		registry: &registry,
		project,
		profile: storage_root()?,
		assets: &assets,
	};
	match command {
		CheckpointTransfer::Export { slot, path } => {
			owner.export(slot, &path)?;
			println!("Exported {}", slot.label());
		}
		CheckpointTransfer::Preview { slot, path } => {
			let review = owner.preview(slot, &path)?;
			println!(
				"{}",
				serde_json::to_string(&review).map_err(|error| error.to_string())?
			);
		}
		CheckpointTransfer::Commit {
			slot,
			path,
			review,
			intent,
		} => {
			owner.import(slot, &path, &review, intent)?;
			println!("Imported {}", slot.label());
		}
	}
	Ok(())
}

fn export_name(slot: SaveSlot, seconds: u64) -> String {
	let number = match slot {
		SaveSlot::One => 1,
		SaveSlot::Two => 2,
		SaveSlot::Three => 3,
	};
	format!("slot-{number}-{seconds}{SUFFIX}")
}

fn ordinary_directory(
	path: &Path,
	create: bool,
	optional: bool,
) -> Result<Option<PathBuf>, String> {
	if create {
		match fs::create_dir(path) {
			Ok(()) => {}
			Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
			Err(error) => return Err(error.to_string()),
		}
	}
	match fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_dir() && !is_linked_package_entry(&metadata) => path
			.canonicalize()
			.map(Some)
			.map_err(|error| error.to_string()),
		Ok(_) => Err("Checkpoint transfer folder must be an ordinary directory".into()),
		Err(error) if error.kind() == ErrorKind::NotFound && optional => Ok(None),
		Err(error) => Err(error.to_string()),
	}
}

fn ordinary_transfer(path: &Path) -> Result<Option<String>, String> {
	let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
		return Ok(None);
	};
	if !name.ends_with(SUFFIX) || name.len() > 96 || name.chars().any(char::is_control) {
		return Ok(None);
	}
	let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
	Ok((metadata.is_file() && !is_linked_package_entry(&metadata)).then(|| name.into()))
}

fn files_in(directory: &Path) -> Result<Vec<TransferFile>, String> {
	let mut files = Vec::new();
	let entries = fs::read_dir(directory).map_err(|error| error.to_string())?;
	for (index, entry) in entries.enumerate() {
		if index == MAX_SCANNED_ENTRIES {
			return Err(
				"Transfer folder has more than 512 entries. Choose a smaller folder.".into(),
			);
		}
		let path = entry.map_err(|error| error.to_string())?.path();
		if let Some(name) = ordinary_transfer(&path)? {
			files.push((name, path));
		}
	}
	files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
	Ok(files
		.into_iter()
		.take(MAX_FILES)
		.enumerate()
		.map(|(index, (name, path))| TransferFile {
			label: transfer_label(index + 1, &name),
			path,
		})
		.collect())
}

fn transfer_label(number: usize, name: &str) -> String {
	// The row number stays visible when names look alike in the bitmap font
	// or the viewport clips a long basename. Keep the original path for I/O.
	let mut basename = name.strip_suffix(SUFFIX).unwrap_or(name).chars();
	let visible: String = basename
		.by_ref()
		.take(22)
		.map(|character| if character.is_ascii() { character } else { '?' })
		.collect();
	let continuation = if basename.next().is_some() { "..." } else { "" };
	format!("Transfer {number:02}: {visible}{continuation}")
}

#[cfg(test)]
#[path = "../tests/unit/checkpoint_transfer_menu.rs"]
mod tests;
