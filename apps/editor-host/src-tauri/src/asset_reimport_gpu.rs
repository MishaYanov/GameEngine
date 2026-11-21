//! Asset publication boundaries shared by the native owner and CPU-only host.
//! Temporary GPU cache entries never replace currently rendered resources until
//! the captured workspace and authored revision have been checked again.
use std::{
	sync::mpsc,
	time::{Duration, Instant},
};

use bridge::editor::EditorSnapshotDto;
use runtime::{asset_catalog::PersistentAsset, assets::project_assets::PreparedAssetChange};

use crate::editor::EditorWorkspace;

pub(crate) type AssetChangeResult = Result<(PersistentAsset, EditorSnapshotDto), String>;

pub(crate) struct AssetChangeRequest {
	pub change: PreparedAssetChange,
	pub generation: u64,
	pub revision: u64,
	pub reply: mpsc::SyncSender<AssetChangeResult>,
	pub deadline: Instant,
}

impl AssetChangeRequest {
	pub fn new(
		change: PreparedAssetChange,
		generation: u64,
		revision: u64,
		reply: mpsc::SyncSender<AssetChangeResult>,
	) -> Self {
		Self {
			change,
			generation,
			revision,
			reply,
			deadline: Instant::now() + Duration::from_secs(30),
		}
	}
	pub fn validate(&self, workspace: &EditorWorkspace) -> Result<(), String> {
		if Instant::now() > self.deadline {
			return Err("Asset operation expired before publication; retry".into());
		}
		validate_context(workspace, &self.change, self.generation, self.revision)
	}
}

pub(crate) fn validate_context(
	workspace: &EditorWorkspace,
	change: &PreparedAssetChange,
	generation: u64,
	revision: u64,
) -> Result<(), String> {
	if workspace.generation != generation {
		return Err("Asset operation belongs to a different open project".into());
	}
	if workspace.backend.session().revision() != revision {
		return Err("Authored scene changed while preparing the asset; retry".into());
	}
	if change.candidate_path().parent() != Some(workspace.asset_root.as_path()) {
		return Err("Asset operation belongs to a different asset root".into());
	}
	workspace.validate_asset_destination(change.destination_path())?;
	workspace
		.backend
		.validate_asset_invalidation()
		.map_err(|error| error.to_string())
}

/// Hold the same workspace guard from final preflight through CPU publication,
/// infallible cache promotion and metadata invalidation. The caller owns GPU
/// upload/waits, which must happen before acquiring that final guard.
pub(crate) fn publish(
	change: PreparedAssetChange,
	workspace: &mut EditorWorkspace,
	promote: impl FnOnce(),
) -> AssetChangeResult {
	workspace.validate_asset_destination(change.destination_path())?;
	workspace
		.backend
		.validate_asset_invalidation()
		.map_err(|error| error.to_string())?;
	workspace
		.backend
		.snapshot()
		.map_err(|error| error.to_string())?;
	let asset = change.publish()?;
	promote();
	let snapshot = workspace.asset_published()?;
	Ok((asset, snapshot))
}

#[cfg(test)]
#[path = "../tests/unit/asset_reimport_gpu.rs"]
mod tests;
