//! Player actions for the opted-in Courtyard checkpoint registry.

use courtyard_gameplay::checkpoint::{
	CheckpointWrite, ContinueAvailability, RestoreSource, SaveSlot, SlotGeneration, SlotInspection,
};

use super::*;

pub(super) fn storage_root() -> Result<PathBuf, String> {
	let directory = std::env::var_os("LOCALAPPDATA")
		.map(PathBuf::from)
		.filter(|path| path.is_absolute())
		.ok_or("Checkpoint storage is unavailable")?;
	Ok(directory)
}

impl GameApp {
	#[cfg(all(test, windows))]
	pub(super) fn checkpoint_availability(&self) -> ContinueAvailability {
		let (Some(registry), Some(project), Ok(root)) =
			(&self.checkpoints, &self.project, storage_root())
		else {
			return ContinueAvailability::Unreadable;
		};
		registry.inspect(&root, project, |scene_id| {
			self.authored_checkpoint_scene(scene_id)
				.map(|(_, document)| document)
		})
	}

	pub(super) fn checkpoint_slots(&self) -> [SlotInspection; 3] {
		let (Some(registry), Some(project), Ok(root)) =
			(&self.checkpoints, &self.project, storage_root())
		else {
			return std::array::from_fn(|_| SlotInspection::default());
		};
		registry.inspect_slots(&root, project, |scene_id| {
			self.authored_checkpoint_scene(scene_id)
				.map(|(_, document)| document)
		})
	}

	pub(super) fn checkpoint_slot(&self, slot: SaveSlot) -> SlotInspection {
		let (Some(registry), Some(project), Ok(root)) =
			(&self.checkpoints, &self.project, storage_root())
		else {
			return SlotInspection::default();
		};
		registry.inspect_slot(&root, slot, project, |scene_id| {
			self.authored_checkpoint_scene(scene_id)
				.map(|(_, document)| document)
		})
	}

	#[cfg(all(test, windows))]
	pub(super) fn save_checkpoint(&self) -> Result<String, String> {
		let expected = self
			.checkpoint_slot(SaveSlot::One)
			.generation
			.ok_or("Checkpoint storage is unavailable")?;
		self.save_checkpoint_slot(SaveSlot::One, &expected)
	}

	pub(super) fn save_checkpoint_slot(
		&self,
		slot: SaveSlot,
		expected: &SlotGeneration,
	) -> Result<String, String> {
		let registry = self
			.checkpoints
			.as_ref()
			.ok_or("This project has no checkpoints")?;
		let scene_id = self.editor.play_scene_id().ok_or("Play is inactive")?;
		let progress = self
			.editor
			.play_progress::<progress::CampaignProgress>()
			.ok_or("Campaign progress is unavailable")?;
		let live = self
			.editor
			.play_document()
			.ok_or("Live scene is unavailable")?;
		let checkpoint = registry
			.active(scene_id, &live, progress)
			.ok_or("Reach an active checkpoint to save")?;
		let project = self.project.as_ref().ok_or("Project is unavailable")?;
		let authored = self
			.checkpoint_base
			.as_deref()
			.ok_or("Authored checkpoint base is unavailable")?;
		registry.save_slot(
			&storage_root()?,
			slot,
			expected,
			CheckpointWrite {
				checkpoint,
				project,
				authored,
				progress,
			},
			|scene_id| {
				self.authored_checkpoint_scene(scene_id)
					.map(|(_, document)| document)
			},
		)?;
		Ok(format!("Saved {}", checkpoint.id))
	}

	#[cfg(all(test, windows))]
	pub(super) fn continue_checkpoint(&mut self) -> Result<String, String> {
		let expected = self
			.checkpoint_slot(SaveSlot::One)
			.generation
			.ok_or("Checkpoint storage is unavailable")?;
		self.continue_checkpoint_slot(SaveSlot::One, &expected, RestoreSource::Current)
	}

	pub(super) fn continue_checkpoint_slot(
		&mut self,
		slot: SaveSlot,
		expected: &SlotGeneration,
		source: RestoreSource,
	) -> Result<String, String> {
		self.scene_load_timing = None;
		self.invalidate_scene_prefetch();
		let mut timing = SceneLoadTiming::new("player", SceneLoadScope::Play, Instant::now());
		let result = self.prepare_checkpoint_restore(slot, expected, source, &mut timing);
		if result.is_ok() {
			self.scene_load_timing = Some(timing);
		} else {
			timing.finish(SceneLoadOutcome::Failed);
		}
		result
	}

	fn prepare_checkpoint_restore(
		&mut self,
		slot: SaveSlot,
		expected: &SlotGeneration,
		source: RestoreSource,
		timing: &mut SceneLoadTiming,
	) -> Result<String, String> {
		let registry = self
			.checkpoints
			.as_ref()
			.ok_or("This project has no checkpoints")?;
		let profile = storage_root()?;
		let project = self.project.as_ref().ok_or("Project is unavailable")?;
		let saved = registry.load_slot_for_restore_with_authored(
			&profile,
			slot,
			expected,
			source,
			project,
			|scene_id| {
				self.authored_checkpoint_scene(scene_id)
					.map(|(_, document)| document)
			},
		)?;
		let scene_id = saved.scene_id().clone();
		let (path, authored) = self.authored_checkpoint_scene(&scene_id)?;
		let (document, progress, status) =
			registry.reconstruct_with_status(&saved, project, &authored)?;
		let document = Arc::new(document);
		let prepared =
			self.editor
				.prepare_restored_game(scene_id, Arc::clone(&document), progress)?;
		self.adopt_prepared_scene_checked(&document, prepared, path, timing, |registry| {
			registry
				.ok_or("Checkpoint registry changed")?
				.verify_slot_generation(&profile, slot, expected)
		})?;
		self.feedback_audio.invalidate();
		self.feedback_captions.clear();
		self.checkpoint_base = Some(Arc::new(authored));
		Ok(if source == RestoreSource::Previous {
			format!("Recovered prior save ({})", saved.checkpoint_id())
		} else if status == ContinueAvailability::CompatibleEarlierRelease {
			format!("Loaded earlier save ({})", saved.checkpoint_id())
		} else {
			format!("Continued from {}", saved.checkpoint_id())
		})
	}

	fn authored_checkpoint_scene(
		&self,
		scene_id: &SceneId,
	) -> Result<(PathBuf, runtime::SceneData), String> {
		let project = self.project.as_ref().ok_or("Project is unavailable")?;
		let assets = self.asset_root.as_ref().ok_or("Assets are unavailable")?;
		let path = play::resolve_scene_path(assets, project.scene_path(scene_id)?)?;
		let authored = scene_io::load_data(&path)?.data;
		Ok((path, authored))
	}
}
