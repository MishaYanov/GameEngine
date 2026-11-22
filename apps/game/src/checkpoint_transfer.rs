//! Shared checkpoint transfer operations for Player Controls and the offline CLI.

use std::{
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
	checkpoint_transfer_history::{Direction, HistoryEntry, TransferHistory},
	local_player_files::{
		ordinary_directory, ordinary_transfer, profile_directory, transfer_entries,
		transfer_label as visible_file_label,
	},
	play::resolve_scene_path,
};

const SUFFIX: &str = ".courtyard-save.json";

pub(super) struct TransferFile {
	pub label: String,
	pub path: PathBuf,
}

#[derive(Clone, Copy)]
enum HistoryStatus {
	Recorded,
	Unavailable,
}

impl HistoryStatus {
	fn menu_message(self, success: String, partial: impl FnOnce() -> String) -> String {
		match self {
			Self::Recorded => success,
			Self::Unavailable => partial(),
		}
	}

	fn print_warning(self) {
		if matches!(self, Self::Unavailable) {
			eprintln!("Checkpoint transfer completed. History is unavailable.");
		}
	}
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

	fn export(&self, slot: SaveSlot, path: &Path) -> Result<HistoryStatus, String> {
		let expected = self
			.registry
			.slot_transfer_generation(&self.profile, slot)?;
		let (bytes, checkpoint) = self.registry.export_slot_transfer_with_checkpoint(
			&self.profile,
			slot,
			&expected,
			self.project,
			|scene| self.authored(scene),
		)?;
		runtime::scene_io::save_new_bytes(path, &bytes)?;
		Ok(self.record(slot, Direction::Export, checkpoint))
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
	) -> Result<HistoryStatus, String> {
		let checkpoint = self.registry.import_slot_transfer_with_checkpoint(
			CheckpointImport {
				profile: &self.profile,
				slot,
				archive,
				review,
				intent,
				project: self.project,
			},
			|scene| self.authored(scene),
		)?;
		Ok(self.record(slot, Direction::Import, checkpoint))
	}

	fn history(&self) -> TransferHistory<'_> {
		TransferHistory::new(&self.profile, self.registry)
	}

	fn record(&self, slot: SaveSlot, direction: Direction, checkpoint: String) -> HistoryStatus {
		match self.history().append(slot, direction, checkpoint) {
			Ok(()) => HistoryStatus::Recorded,
			Err(error) => {
				engine_logger::warn!(target: "checkpoint_transfer_history", "Checkpoint transfer completed; history unavailable: {error}");
				HistoryStatus::Unavailable
			}
		}
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
		ordinary_directory(&profile, false, false)?;
		profile_directory(&profile, &["GameEngine", "Checkpoint Transfers"], create)
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
		let history = owner.export(slot, &directory.join(&name))?;
		Ok(history.menu_message(format!("Exported {name}"), || {
			"Exported checkpoint. History unavailable.".into()
		}))
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
		let history = self
			.transfer_owner()?
			.import(slot, &file.path, &review, intent)?;
		Ok(history.menu_message(
			format!("Imported {}. Select Continue to resume.", slot.label()),
			|| format!("Imported {}. History unavailable.", slot.label()),
		))
	}

	pub(super) fn checkpoint_transfer_history(
		&self,
		slot: SaveSlot,
	) -> Result<Vec<HistoryEntry>, String> {
		self.transfer_owner()?.history().slot(slot)
	}

	fn check_transfer_selection(&self, file: &TransferFile) -> Result<(), String> {
		let directory = self
			.checkpoint_transfer_directory(false)?
			.ok_or("Transfer folder is unavailable")?;
		if file.path.parent() != Some(directory.as_path())
			|| ordinary_transfer(&file.path, SUFFIX)?.is_none()
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
			let history = owner.export(slot, &path)?;
			println!("Exported {}", slot.label());
			history.print_warning();
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
			let history = owner.import(slot, &path, &review, intent)?;
			println!("Imported {}", slot.label());
			history.print_warning();
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

fn files_in(directory: &Path) -> Result<Vec<TransferFile>, String> {
	Ok(transfer_entries(directory, SUFFIX)?
		.into_iter()
		.enumerate()
		.map(|(index, (name, path))| TransferFile {
			label: transfer_label(index + 1, &name),
			path,
		})
		.collect())
}

fn transfer_label(number: usize, name: &str) -> String {
	visible_file_label("Transfer", number, name, SUFFIX)
}

#[cfg(test)]
#[path = "../tests/unit/checkpoint_transfer_menu.rs"]
mod tests;
