//! Installed Player's no-window preset transfer command adapter.

use crate::player_controls::input_preset::PresetTransfer;
use game::launch::InputPresetCommand;
use runtime::project::ProjectData;
use std::path::Path;

pub(crate) fn execute(
	command: InputPresetCommand,
	manifest: Option<&Path>,
	project: Option<&ProjectData>,
) -> Result<(), String> {
	let message = "Input presets require an installed project.json";
	let transfer = PresetTransfer::new(manifest.ok_or(message)?, project.ok_or(message)?)?;
	match command {
		InputPresetCommand::Export(path) => {
			transfer.export(&path)?;
			println!("Exported input preset");
		}
		InputPresetCommand::Preview(path) => {
			let review = transfer.preview(&path)?;
			println!(
				"{}",
				serde_json::to_string(&review).map_err(|error| error.to_string())?
			);
		}
		InputPresetCommand::Apply {
			path,
			source_sha256,
			destination_sha256,
		} => {
			transfer.apply(&path, source_sha256, destination_sha256)?;
			println!("Applied input preset");
		}
	}
	Ok(())
}
