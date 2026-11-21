use super::*;

pub(super) fn is_linked_package_entry(metadata: &std::fs::Metadata) -> bool {
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		if metadata.file_attributes() & 0x400 != 0 {
			return true;
		}
	}
	metadata.file_type().is_symlink()
}

pub(super) fn resolve_scene_path(asset_root: &Path, relative: &str) -> Result<PathBuf, String> {
	let root = asset_root
		.canonicalize()
		.map_err(|error| error.to_string())?;
	let mut path = root.clone();
	for component in Path::new(relative).components() {
		let std::path::Component::Normal(name) = component else {
			return Err("Scene path must be a relative asset path".into());
		};
		path.push(name);
		let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
		if is_linked_package_entry(&metadata) {
			return Err("Scene path includes a linked package entry".into());
		}
	}
	let scene = path.canonicalize().map_err(|error| error.to_string())?;
	if !scene.starts_with(root) || !scene.is_file() {
		return Err("Scene must resolve to a regular file inside the asset root".into());
	}
	Ok(scene)
}

impl GameApp {
	pub(super) fn update_player(&mut self, delta_seconds: f32) {
		let completed = self.game_completed();
		// The completion message promises F10 even when gameplay normally owns it.
		if (completed && self.input.just_pressed(Key::F10))
			|| self.player_shortcut_pressed(Key::F10)
		{
			let result = self.restart_player_game();
			report_update_result(
				"player_new_game",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		if completed {
			return;
		}
		if !self.installed_project
			&& self.player_shortcut_pressed(Key::F8)
			&& self.editor.pending_play_scene().is_none()
		{
			let result = self.cycle_player_scene();
			report_update_result(
				"player_scene_switch",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		if !self.installed_project
			&& self.player_shortcut_pressed(Key::F7)
			&& self
				.editor
				.play_progress::<progress::CampaignProgress>()
				.is_some()
		{
			let result = runtime::game_progress::ProgressAction::new::<progress::CampaignProgress>(
				progress::ProgressAction::AwardScore { amount: 1 },
			)
			.and_then(|action| self.editor.apply_play_progress(&[action]));
			report_update_result(
				"player_progress",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		self.update_keyboard_camera(delta_seconds);
		let gate_is_open = |editor: &SceneEditor| {
			editor
				.play_progress::<progress::CampaignProgress>()
				.is_some_and(|progress| progress.route_gate_open)
		};
		let previously_open = gate_is_open(&self.editor);
		let result = self.tick_play(delta_seconds);
		if result.is_ok() && !previously_open && gate_is_open(&self.editor) {
			feedback_audio::play_gate_cue();
		}
		report_update_result(
			"player_simulation",
			result,
			&mut self.play_update_error_reported,
		);
	}

	pub(super) fn tick_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		let result = self.advance_play(delta_seconds);
		if self
			.orbit_capture
			.as_ref()
			.is_some_and(|owner| !self.editor.owns_camera_input(owner) || self.game_completed())
		{
			self.set_mouse_capture(false);
		}
		result
	}

	pub(super) fn advance_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		if self.input.just_pressed(Key::F8)
			&& let Some(scene_id) = self.editor.pending_play_scene().cloned()
		{
			let result = self.install_play_scene(scene_id, false);
			if result.is_err() {
				self.update_selection_title();
			}
			return result;
		}
		if self.editor.has_runtime_instances() {
			self.renderer
				.as_ref()
				.ok_or("Renderer is unavailable")?
				.wait_idle()?;
		}
		let mut outcome = self.editor.update_play_with_gamepad(
			&mut self.scene,
			&self.input,
			self.play_gamepad.sample(),
			delta_seconds,
		)?;
		loop {
			if let Some(camera) = self.editor.play_rig_camera() {
				self.camera = camera;
			}
			if let Some(scene_id) = outcome.requested_scene {
				let result = self.install_play_scene(scene_id, false);
				if result.is_err() {
					self.update_selection_title();
				}
				return result;
			}
			self.fulfill_runtime_spawns()?;
			// synchronize_world can retire runtime models; submitted frames have
			// completed before each mutating tick above/below.
			if outcome.changes.topology_changed
				&& let Some(assets) = &mut self.asset_manager
			{
				assets.prune_unused();
			}
			if outcome.pending_steps == 0 {
				break;
			}
			if self.editor.has_runtime_instances() {
				self.renderer
					.as_ref()
					.ok_or("Renderer is unavailable")?
					.wait_idle()?;
			}
			outcome = self.editor.drain_play(&mut self.scene)?;
		}
		Ok(())
	}

	pub(super) fn fulfill_runtime_spawns(&mut self) -> Result<(), String> {
		if self.editor.simulation_paused() {
			return Ok(());
		}
		for request in self.editor.pending_runtime_spawns() {
			let result = (|| {
				let root = self
					.asset_root
					.as_ref()
					.ok_or("Asset root is unavailable")?;
				let (prefab, fingerprint) =
					bridge::runtime_prefab::load(root, &request.prefab_path)?;
				let mut prepared = self.editor.prepare_runtime_spawn(
					&prefab,
					&fingerprint,
					request.position,
					Some(request.id),
				)?;
				let data = self.editor.runtime_spawn_document(&prepared)?;
				let renderer = self.renderer.as_ref().ok_or("Renderer is unavailable")?;
				let assets = self
					.asset_manager
					.as_mut()
					.ok_or("Assets are unavailable")?;
				let candidate = scene_io::instantiate(&data, assets, renderer);
				renderer.wait_idle()?;
				let (scene, _) = candidate?;
				let keys: std::collections::HashSet<_> = prepared
					.entities()
					.iter()
					.map(|entity| entity.key.clone())
					.collect();
				prepared.set_visibility_capabilities(
					scene
						.visibility_capabilities()
						.into_iter()
						.filter(|key| keys.contains(key))
						.collect(),
				)?;
				self.editor.commit_runtime_spawn(prepared)?;
				let mut scene = scene;
				if let Some(bank) = self.editor.animations() {
					scene.synchronize_animation(bank)?;
				}
				self.scene = scene;
				Ok::<(), String>(())
			})();
			if let Err(error) = result {
				self.editor.reject_runtime_spawn(request.id, &error)?;
				engine_logger::error!(target:"game", "Runtime spawn rejected: operation=prepare_and_admit_runtime_spawn");
			}
			if let Some(assets) = &mut self.asset_manager {
				assets.prune_unused();
			}
		}
		Ok(())
	}

	pub(super) fn player_shortcut_pressed(&self, key: Key) -> bool {
		self.input.just_pressed(key) && !self.editor.play_input_binds_key(key)
	}

	pub(super) fn game_completed(&self) -> bool {
		self.editor
			.play_status()
			.and_then(|status| status.gameplay)
			.is_some_and(|status| status.completed)
	}

	pub(super) fn toggle_play(&mut self) -> Result<(), String> {
		if self.authored.is_some() {
			self.stop_play()
		} else {
			self.start_play()
		}
	}

	pub(super) fn start_play(&mut self) -> Result<(), String> {
		if self.authored.is_some() {
			return Ok(());
		}
		self.cancel_drag();
		let mut play_scene = self.scene.clone();
		self.editor.start_play(&mut play_scene)?;
		let authored_scene = std::mem::replace(&mut self.scene, play_scene);
		self.authored = Some(AuthoredScene {
			scene: authored_scene,
			camera: self.camera.clone(),
		});
		self.finish_mode_change();
		Ok(())
	}

	pub(super) fn stop_play(&mut self) -> Result<(), String> {
		self.wait_for_gpu()?;
		let Some(authored) = self.authored.as_mut() else {
			return Ok(());
		};
		self.editor.stop_play(&mut authored.scene)?;
		let authored = self.authored.take().expect("authored scene was present");
		self.scene = authored.scene;
		self.camera = authored.camera;
		self.finish_mode_change();
		Ok(())
	}

	pub(super) fn finish_mode_change(&mut self) {
		self.tools.panel.edit = None;
		self.set_mouse_capture(false);
		self.input.suppress_held();
		self.editor.end_gesture();
		self.clock.reset();
		self.update_selection_title();
	}

	pub(super) fn cycle_player_scene(&mut self) -> Result<(), String> {
		let current = self
			.editor
			.play_scene_id()
			.cloned()
			.ok_or("Play mode is not active")?;
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?;
		let mut scene_ids: Vec<_> = project
			.scenes
			.iter()
			.map(|scene| scene.id().clone())
			.collect();
		scene_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
		if scene_ids.len() < 2 {
			return Ok(());
		}
		let current_index = scene_ids
			.iter()
			.position(|id| id == &current)
			.ok_or("Running scene is absent from the project catalog")?;
		let next = scene_ids[(current_index + 1) % scene_ids.len()].clone();
		self.switch_player_scene(next)
	}

	pub(super) fn switch_player_scene(&mut self, scene_id: SceneId) -> Result<(), String> {
		self.install_play_scene(scene_id, false)
	}

	pub(super) fn restart_player_game(&mut self) -> Result<(), String> {
		let startup = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?
			.startup_scene_id
			.clone();
		self.install_play_scene(startup, true)
	}

	pub(super) fn install_play_scene(
		&mut self,
		scene_id: SceneId,
		new_game: bool,
	) -> Result<(), String> {
		let current = self
			.editor
			.play_scene_id()
			.ok_or("Play mode is not active")?;
		if !new_game && current == &scene_id {
			return Ok(());
		}
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?;
		let relative_path = project.scene_path(&scene_id)?;
		let asset_root = self
			.asset_root
			.as_ref()
			.ok_or("Asset root is not initialized")?;
		let scene_path = resolve_scene_path(asset_root, relative_path)?;
		let parsed = scene_io::load_data(&scene_path)?;
		let document = Arc::new(parsed.data);
		let prepared = if new_game {
			self.editor
				.prepare_new_game(scene_id, Arc::clone(&document))?
		} else {
			self.editor
				.prepare_play_scene(scene_id, Arc::clone(&document))?
		};
		self.adopt_prepared_scene(&document, prepared, scene_path)?;
		if self.checkpoints.is_some() {
			self.checkpoint_base = Some(document);
		}
		Ok(())
	}

	pub(super) fn adopt_prepared_scene(
		&mut self,
		document: &runtime::scene_data::SceneData,
		prepared: runtime::scene_editor::PreparedScene,
		scene_path: PathBuf,
	) -> Result<(), String> {
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;
		let assets = self
			.asset_manager
			.as_mut()
			.ok_or("Assets are not initialized")?;
		let candidate = scene_io::instantiate(document, assets, renderer);

		// Both scenes share one cache and budget until the candidate commits.
		// Completion also makes cache-only allocations from rejected loads safe to prune.
		renderer.wait_idle()?;
		let installed = candidate.and_then(|(scene, camera)| {
			self.editor
				.commit_play_scene(prepared, &mut self.input, self.authored.is_some())?;
			// No fallible operations between committing the CPU world and its view.
			self.scene = scene;
			self.camera = self.editor.play_rig_camera().unwrap_or(camera);
			Ok(())
		});
		assets.prune_unused();
		installed?;
		self.play_gamepad.reset();
		self.set_mouse_capture(false);
		if self.standalone_runtime {
			self.scene_path = Some(scene_path);
		}
		self.clock.reset();
		self.next_frame_deadline = None;
		self.play_update_error_reported = false;
		self.update_selection_title();
		if let Some(window) = self.window.as_ref() {
			window.request_redraw();
		}
		Ok(())
	}
}
