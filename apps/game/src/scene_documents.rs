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

	pub(super) fn prepare_editor(
		loaded: &mut scene_io::LoadedScene,
	) -> Result<SceneEditor, String> {
		if loaded.source_schema_version == 1
			&& let [left_robot, right_robot, ..] = loaded.entity_ids.as_slice()
		{
			loaded.scene.set_binding("robot_a", *left_robot)?;
			loaded.scene.set_binding("robot_b", *right_robot)?;
		}
		let data = loaded.scene.to_data(&loaded.camera)?;
		let project = ProjectData::for_scene("Game Engine", "scenes/robots.json", &data)?;
		SceneEditor::for_project(&loaded.scene, &loaded.camera, project)
	}

	pub(super) fn reload_scene(&mut self) -> Result<(), String> {
		let old_path = self
			.scene_path
			.clone()
			.ok_or("Scene path is not initialized")?;
		let asset_root = self
			.asset_root
			.as_deref()
			.ok_or("Asset root is not initialized")?;
		let project = self
			.project_path
			.as_ref()
			.map(ProjectData::load)
			.transpose()?
			.or_else(|| self.project.clone());
		let path = match project.as_ref() {
			Some(project) => asset_root.join(project.startup_scene_path()?),
			None => old_path,
		};
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;

		let mut loaded = scene_io::load(&path, asset_root, renderer)?;
		let mut editor = Self::prepare_editor(&mut loaded)?;
		if let Some(project) = project.as_ref() {
			editor.set_initial_project(project.clone())?;
		}
		editor.set_module_resolver(Arc::new(progress::resolve))?;
		if self.standalone_runtime {
			editor.start_play(&mut loaded.scene)?;
		}
		let saved = loaded.scene.to_data(&loaded.camera)?;

		// Drawing can still reference the old meshes and textures. Finish all
		// GPU work before assignments release the old scene, then its cache.
		renderer.wait_idle()?;
		self.scene = loaded.scene;
		self.authored = None;
		self.spawn_template = None;
		self.tools.templates.clear();
		self.tools.drag = None;
		self.tools.saved = Some(saved);
		if let Some(profile) = &self.input_profile_override {
			editor.set_play_input_profile(profile.clone(), &mut self.input);
		}
		self.editor = editor;
		self.asset_manager = Some(loaded.assets);
		self.camera = loaded.camera;
		self.project = project;
		self.scene_path = Some(path.clone());
		self.finish_mode_change();
		engine_logger::info!("Reloaded scene: {}", path.display());
		Ok(())
	}
}
