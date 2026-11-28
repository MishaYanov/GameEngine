//! Cinder Oath's portable arena-entry checkpoint format and installed-package identity.

pub(crate) mod audit;
mod store;

pub(crate) use store::{Attempt, PracticeStore, Preflight, Review, TransferFile, publish_transfer};

use crate::{GameApp, local_player_files::read_bounded};
use gameplay_catalog::{
	CINDER_OATH_ID,
	cinder_oath::{CinderProgress, intro_route},
};
use runtime::{
	project::{MAX_PROJECT_BYTES, ProjectData},
	scene_catalog::SceneId,
	scene_data::parse_json,
	scene_editor::PreparedScene,
	scene_io::read_confined_scene_source,
	scene_load_timing::{Outcome as SceneLoadOutcome, SceneLoadTiming, Scope as SceneLoadScope},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
	time::Instant,
};

pub(crate) const CHECKPOINT_SUFFIX: &str = ".checkpoint.json";
const CHECKPOINT_ID: &str = "cinder-oath-arena-entry";
const PROJECT_ID: &str = "cinder-oath";
const SLOT_NUMBER: u8 = 1;
const MAX_CHECKPOINT_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
	schema_version: u32,
	project_id: String,
	checkpoint_id: String,
	project_sha256: String,
	arena_sha256: String,
}

impl Checkpoint {
	fn arena_entry(project: &[u8], arena: &[u8]) -> Self {
		Self {
			schema_version: 1,
			project_id: PROJECT_ID.into(),
			checkpoint_id: CHECKPOINT_ID.into(),
			project_sha256: format!("{:x}", Sha256::digest(project)),
			arena_sha256: format!("{:x}", Sha256::digest(arena)),
		}
	}

	pub(crate) fn id(&self) -> &'static str {
		CHECKPOINT_ID
	}

	fn is_well_formed(&self) -> bool {
		self.schema_version == 1
			&& valid_identifier(&self.project_id)
			&& valid_identifier(&self.checkpoint_id)
			&& valid_digest(&self.project_sha256)
			&& valid_digest(&self.arena_sha256)
	}

	fn is_cinder_entry(&self) -> bool {
		self.is_well_formed()
			&& self.project_id == PROJECT_ID
			&& self.checkpoint_id == CHECKPOINT_ID
	}

	fn check_compatibility(&self, arena: &Self) -> Result<(), PreflightBlock> {
		if self.project_id != arena.project_id || self.project_sha256 != arena.project_sha256 {
			return Err(PreflightBlock::IncompatibleProject);
		}
		if self.checkpoint_id != arena.checkpoint_id || self.arena_sha256 != arena.arena_sha256 {
			return Err(PreflightBlock::InvalidAuthoredScene);
		}
		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreflightBlock {
	InvalidArchive,
	IncompatibleProject,
	InvalidAuthoredScene,
	DestinationUnavailable,
}

impl PreflightBlock {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::InvalidArchive => "Invalid checkpoint archive",
			Self::IncompatibleProject => "Installed project does not match",
			Self::InvalidAuthoredScene => "Authored arena does not match",
			Self::DestinationUnavailable => "Practice slot is unavailable",
		}
	}
}

pub(crate) struct ArenaEntry {
	pub(crate) checkpoint: Checkpoint,
	prepared: PreparedScene,
	document: Arc<runtime::SceneData>,
	path: PathBuf,
}

impl GameApp {
	pub(crate) fn supports_checkpoint_transfer(&self) -> bool {
		self.installed_project
			&& self
				.project
				.as_ref()
				.and_then(|project| project.gameplay_module.as_deref())
				== Some(CINDER_OATH_ID)
	}

	pub(crate) fn checkpoint_arena_scene(&self) -> Option<SceneId> {
		self.supports_checkpoint_transfer().then_some(())?;
		intro_route(self.project.as_ref()?)
			.ok()
			.flatten()
			.map(|(_, arena)| arena)
	}

	pub(crate) fn prepare_checkpoint_arena(&self) -> Result<ArenaEntry, PreflightBlock> {
		let scene = self
			.checkpoint_arena_scene()
			.ok_or(PreflightBlock::IncompatibleProject)?;
		let manifest = self
			.project_path
			.as_deref()
			.ok_or(PreflightBlock::IncompatibleProject)?;
		let project = read_installed_manifest(manifest)?;
		if self.project.as_ref() != Some(&project) {
			return Err(PreflightBlock::IncompatibleProject);
		}
		let normalized_manifest = project
			.to_json()
			.map_err(|_| PreflightBlock::IncompatibleProject)?;
		let (path, source) = (|| {
			let root = self.asset_root.as_deref().ok_or("Player not initialized")?;
			read_confined_scene_source(root, Path::new(project.scene_path(&scene)?))
		})()
		.map_err(|_| PreflightBlock::InvalidAuthoredScene)?;
		let document = Arc::new(
			parse_json(&source)
				.map_err(|_| PreflightBlock::InvalidAuthoredScene)?
				.data,
		);
		let prepared = self
			.editor
			.prepare_restored_game(scene, Arc::clone(&document), CinderProgress::arena_entry())
			.map_err(|_| PreflightBlock::InvalidAuthoredScene)?;
		Ok(ArenaEntry {
			checkpoint: Checkpoint::arena_entry(normalized_manifest.as_bytes(), source.as_bytes()),
			prepared,
			document,
			path,
		})
	}

	pub(crate) fn resume_checkpoint_arena(
		&mut self,
		checkpoint: &Checkpoint,
	) -> Result<(), String> {
		if self.play_trace.is_some() {
			return Err("Practice entry is unavailable while recording".into());
		}
		let arena = self
			.prepare_checkpoint_arena()
			.map_err(PreflightBlock::label)?;
		checkpoint
			.check_compatibility(&arena.checkpoint)
			.map_err(PreflightBlock::label)?;
		self.scene_load_timing = None;
		let mut timing = SceneLoadTiming::new("player", SceneLoadScope::Play, Instant::now());
		match self.adopt_prepared_scene(&arena.document, arena.prepared, arena.path, &mut timing) {
			Ok(()) => {
				self.scene_load_timing = Some(timing);
				self.feedback_audio.invalidate();
				self.feedback_captions.clear();
				Ok(())
			}
			Err(error) => {
				timing.finish(SceneLoadOutcome::Failed);
				Err(error)
			}
		}
	}
}

fn read_installed_manifest(path: &Path) -> Result<ProjectData, PreflightBlock> {
	let invalid = PreflightBlock::IncompatibleProject;
	let bytes = read_bounded(path, MAX_PROJECT_BYTES)
		.map_err(|_| invalid)?
		.ok_or(invalid)?;
	ProjectData::from_json(std::str::from_utf8(&bytes).map_err(|_| invalid)?).map_err(|_| invalid)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
	Sha256::digest(bytes).into()
}

fn valid_identifier(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 64
		&& value.bytes().all(|byte| {
			byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
		})
}

fn valid_digest(value: &str) -> bool {
	value.len() == 64
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "../tests/unit/cinder_checkpoint.rs"]
mod tests;
