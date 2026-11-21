//! Player actions for the opted-in Courtyard checkpoint registry.

use courtyard_gameplay::checkpoint::ContinueAvailability;

use super::*;

fn storage_root() -> Result<PathBuf, String> {
	let directory = std::env::var_os("LOCALAPPDATA")
		.map(PathBuf::from)
		.filter(|path| path.is_absolute())
		.ok_or("Checkpoint storage is unavailable")?;
	Ok(directory)
}

impl GameApp {
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

	pub(super) fn save_checkpoint(&self) -> Result<String, String> {
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
		registry.save(&storage_root()?, checkpoint, project, &authored, progress)?;
		Ok(format!("Saved {}", checkpoint.id))
	}

	pub(super) fn continue_checkpoint(&mut self) -> Result<String, String> {
		let registry = self
			.checkpoints
			.as_ref()
			.ok_or("This project has no checkpoints")?;
		let saved = registry
			.load(&storage_root()?)?
			.ok_or("No saved checkpoint")?;
		let project = self.project.as_ref().ok_or("Project is unavailable")?;
		let scene_id = saved.scene_id().clone();
		let (path, authored) = self.authored_checkpoint_scene(&scene_id)?;
		let (document, progress, status) =
			registry.reconstruct_with_status(&saved, project, &authored)?;
		let document = Arc::new(document);
		let prepared =
			self.editor
				.prepare_restored_game(scene_id, Arc::clone(&document), progress)?;
		self.adopt_prepared_scene(&document, prepared, path)?;
		self.checkpoint_base = Some(Arc::new(authored));
		Ok(
			if status == ContinueAvailability::CompatibleEarlierRelease {
				format!("Loaded earlier save ({})", saved.checkpoint_id())
			} else {
				format!("Continued from {}", saved.checkpoint_id())
			},
		)
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
