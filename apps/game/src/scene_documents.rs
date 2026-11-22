use super::*;

impl GameApp {
	pub(super) fn save_scene(&mut self) -> Result<(), String> {
		let path = self
			.scene_path
			.as_deref()
			.ok_or("Scene path is not initialized")?;
		let (scene, camera) = match self.authored.as_ref() {
			Some(authored) => (&authored.scene, &authored.camera),
			None => (&self.scene, &self.camera),
		};
		let data = scene.to_data(camera)?;
		let project = self
			.editor
			.project()
			.cloned()
			.ok_or("Project configuration is not initialized")?;
		project.validate_scene(&data)?;
		let project_path = self
			.project_path
			.clone()
			.or_else(|| {
				self.asset_root
					.as_ref()
					.and_then(|root| root.parent())
					.map(|root| root.join("project.json"))
			})
			.ok_or("Project path is not initialized")?;
		scene_io::save_data(path, &data)?;
		scene_io::save_project_data(&project_path, &project).map_err(|error| {
			format!(
				"Scene was saved, but project settings were not saved: {error}. The document remains dirty."
			)
		})?;
		self.project_path = Some(project_path);
		self.project = Some(project);
		self.tools.saved = Some(data);
		self.editor.mark_saved()?;
		engine_logger::info!("Saved scene: {}", path.display());
		self.update_selection_title();
		Ok(())
	}

	pub(super) fn reload_scene(&mut self) -> Result<(), String> {
		let asset_root = self
			.asset_root
			.as_deref()
			.ok_or("Asset root is not initialized")?;
		let project = self
			.project_path
			.as_ref()
			.map(ProjectData::load)
			.transpose()?
			.or_else(|| self.project.clone())
			.ok_or("Project configuration is not initialized")?;
		let path = play::resolve_scene_path(asset_root, project.startup_scene_path()?)?;
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;

		let mut loaded = scene_io::load(&path, asset_root, renderer)?;
		let mut editor = SceneEditor::for_project(&loaded.scene, &loaded.camera, project.clone())?;
		editor.set_module_resolver(Arc::new(gameplay_catalog::resolve))?;
		let audio_catalog = bridge::feedback_audio::load(asset_root)?;
		let playback = editor
			.project()
			.ok_or("Project configuration is unavailable")?;
		if let Some(catalog) = &audio_catalog {
			catalog.mix().validate_scenes(&playback.scenes)?;
		}
		let audio = feedback_audio::FeedbackAudio::new(
			audio_catalog,
			!self.smoke_test,
			self.player_controls.feedback_mix(),
			&playback.startup_scene_id,
		)?;
		if self.standalone_runtime {
			editor.start_play(&mut loaded.scene)?;
		}
		let saved = loaded.scene.to_data(&loaded.camera)?;

		// Drawing can still reference the old meshes and textures. Finish all
		// GPU work before assignments release the old scene, then its cache.
		renderer.wait_idle()?;
		self.scene = loaded.scene;
		self.authored = None;
		self.tools.templates.clear();
		self.tools.drag = None;
		self.tools.saved = Some(saved);
		if let Some(profile) = &self.input_profile_override {
			editor.set_play_input_profile(profile.clone(), &mut self.input);
		}
		self.editor = editor;
		self.feedback_audio = audio;
		self.asset_manager = Some(loaded.assets);
		self.camera = loaded.camera;
		self.project = Some(project);
		self.scene_path = Some(path.clone());
		self.finish_mode_change();
		engine_logger::info!("Reloaded scene: {}", path.display());
		Ok(())
	}
}
