//! Offline commands for transferring one checkpoint without a running game.

use std::path::Path;

use courtyard_gameplay::checkpoint::{CheckpointImport, CheckpointRegistry};
use game::launch::CheckpointTransfer;
use runtime::project::ProjectData;

use super::{checkpoint_controls::storage_root, play::resolve_scene_path};

pub(super) fn execute(
	command: CheckpointTransfer,
	project: Option<&ProjectData>,
	install_root: &Path,
) -> Result<(), String> {
	let project = project.ok_or("Checkpoint transfer requires an installed project.json")?;
	let registry = CheckpointRegistry::from_project(project)?
		.ok_or("The installed project has no checkpoint registry")?;
	let profile = storage_root()?;
	let assets = install_root.join("assets");
	let load_authored = |scene: &runtime::scene_catalog::SceneId| {
		let path = resolve_scene_path(&assets, project.scene_path(scene)?)?;
		Ok(runtime::scene_io::load_data(&path)?.data)
	};
	match command {
		CheckpointTransfer::Export { slot, path } => {
			let expected = registry.slot_transfer_generation(&profile, slot)?;
			let bytes =
				registry.export_slot_transfer(&profile, slot, &expected, project, load_authored)?;
			runtime::scene_io::save_new_bytes(&path, &bytes)?;
			println!("Exported {}", slot.label());
		}
		CheckpointTransfer::Preview { slot, path } => {
			let review =
				registry.preview_slot_transfer(&profile, slot, &path, project, load_authored)?;
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
			registry.import_slot_transfer(
				CheckpointImport {
					profile: &profile,
					slot,
					archive: &path,
					review: &review,
					intent,
					project,
				},
				load_authored,
			)?;
			println!("Imported {}", slot.label());
		}
	}
	Ok(())
}
