use super::*;

#[path = "scene_switch_benchmark.rs"]
mod scene_switch_benchmark;
use runtime::scene_load_timing::{BenchmarkPhase, SceneLoadResources};
use scene_switch_benchmark::{SceneSwitchSample, micros};

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

impl GameApp {
	pub(super) fn update_player(&mut self, delta_seconds: f32) {
		let completed = self.game_completed();
		// The completion message promises F10 even when gameplay normally owns it.
		if self.play_trace.is_none()
			&& ((completed && self.input.just_pressed(Key::F10))
				|| self.player_shortcut_pressed(Key::F10))
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
		if completed
			&& !self.cinder_victory_animation_active()
			&& self.editor.pending_scene_transition().is_none()
			&& self.failed_scene_transition.is_none()
		{
			return;
		}
		if self.play_trace.is_none()
			&& !self.installed_project
			&& self.failed_scene_transition.is_none()
			&& self.world_streaming_status.is_none()
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
		self.update_keyboard_camera(delta_seconds);
		let result = self.tick_play(delta_seconds);
		report_update_result(
			"player_simulation",
			result,
			&mut self.play_update_error_reported,
		);
	}

	pub(super) fn tick_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		let result = self.advance_play(delta_seconds);
		if self.orbit_capture.as_ref().is_some_and(|owner| {
			!self.editor.owns_camera_input(owner)
				|| self.game_completed()
				|| self.cinder_phase() == Some(gameplay_catalog::cinder_oath::Phase::Defeat)
		}) {
			self.set_mouse_capture(false);
		}
		result
	}

	pub(super) fn advance_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		if self.drive_authored_scene_transition()? {
			return Ok(());
		}
		self.update_world_streaming()?;
		if self.play_trace.is_none()
			&& self.input.just_pressed(Key::F8)
			&& let Some(scene_id) = self.editor.pending_play_scene().cloned()
		{
			let result = self.install_play_scene(scene_id, false);
			if result.is_err() {
				self.update_selection_title();
			}
			return result;
		}
		if self.play_trace.is_none() && self.input.just_pressed(Key::Enter) {
			self.editor.skip_play_sequence();
		}
		let pad = self.logical_gamepad();
		let physical = self.physical_gamepad();
		let was_paused = self.editor.simulation_paused();
		let mut prior_combat = self.combat_sound_facts();
		let pointer_input = self.pointer_attack_input();
		let input = pointer_input.as_ref().unwrap_or(&self.input);
		let mut outcome = self.editor.update_play_with_draw(
			&mut self.scene,
			input,
			(pad, physical),
			delta_seconds,
			self.renderer.as_ref(),
		)?;
		let paused = self.editor.simulation_paused();
		if paused != was_paused {
			self.reset_gamepad_input();
		}
		let input = pointer_input.as_ref().unwrap_or(&self.input);
		if let Some(trace) = self.play_trace.as_mut() {
			trace.record_paused(paused);
			if let Some(map) = self.editor.play_input_action_map() {
				let sample = if paused != was_paused {
					Default::default()
				} else {
					physical
				};
				trace.record_input_actions(map.held_keyboard_codes(input), sample);
			} else {
				let sample = if paused != was_paused {
					GamepadSample::default()
				} else {
					pad
				};
				trace.record_controls(input, sample);
			}
		}
		loop {
			if outcome.steps > 0
				&& let (Some(prior), Some(after)) = (prior_combat, self.combat_sound_facts())
				&& let Some(cue) = self.project_sound.emit_combat_change(
					prior,
					after,
					self.player_controls.feedback_mix(),
					|cue| {
						outcome.audio_onsets.iter().any(|onset| {
							onset
								.replaces_cue
								.is_some_and(|replacement| replacement.id() == cue.id())
						})
					},
				) && self.standalone_runtime
				&& self.player_controls.display().cue_captions
			{
				self.feedback_captions.push(cue, Instant::now());
			}
			if !outcome.audio_onsets.is_empty()
				&& let Some(status) = self.editor.play_status()
			{
				self.scene_audio.queue_onsets(
					runtime::scene_audio::SceneAudioOwner::from_play(&status),
					std::mem::take(&mut outcome.audio_onsets),
				);
			}
			self.scene_audio.settle_sequences(&outcome.sequence_events);
			if let Some(trace) = self.play_trace.as_mut() {
				trace.record_steps(outcome.steps);
			}
			if let Some(camera) = self.editor.play_rig_camera() {
				self.camera = camera;
			}
			if let Some(scene_id) = outcome.requested_scene {
				if self.drive_authored_scene_transition()? {
					return Ok(());
				}
				let result = self.install_play_scene(scene_id, false);
				if result.is_err() {
					self.update_selection_title();
				}
				return result;
			}
			self.fulfill_runtime_spawns()?;
			if outcome.changes.topology_changed
				&& let Some(assets) = &mut self.asset_manager
			{
				assets.prune_unused();
			}
			if outcome.pending_steps == 0 {
				break;
			}
			prior_combat = self.combat_sound_facts();
			outcome = self
				.editor
				.drain_play_with_draw(&mut self.scene, self.renderer.as_ref())?;
		}
		Ok(())
	}

	fn combat_sound_facts(&self) -> Option<project_sound::CombatSound> {
		self.editor
			.play_progress::<gameplay_catalog::cinder_oath::CinderProgress>()
			.map(project_sound::CombatSound::from)
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
		let scene_mix = self
			.editor
			.project()
			.map(|project| {
				let scene = &project.startup_scene_id;
				self.feedback_audio
					.prepare_scene_mix(self.player_controls.feedback_mix(), scene)
					.map(|mix| (scene.clone(), mix))
			})
			.transpose()?;
		let mut play_scene = self.scene.clone();
		self.editor.start_play(&mut play_scene)?;
		if let Some((scene, mix)) = scene_mix {
			self.feedback_audio.commit_scene_mix(scene, mix);
		}
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
		self.invalidate_scene_prefetch();
		self.feedback_audio.invalidate();
		self.feedback_captions.clear();
		self.project_sound.stop_effect();
		self.update_project_sound();
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
		self.install_play_scene(startup, true)?;
		self.feedback_audio.invalidate();
		self.feedback_captions.clear();
		Ok(())
	}

	pub(super) fn install_play_scene(
		&mut self,
		scene_id: SceneId,
		new_game: bool,
	) -> Result<(), String> {
		self.install_timed_player_scene(scene_id, new_game, SceneLoadScope::Play)
	}

	fn install_timed_player_scene(
		&mut self,
		scene_id: SceneId,
		new_game: bool,
		scope: SceneLoadScope,
	) -> Result<(), String> {
		self.install_observed_player_scene(scene_id, new_game, scope, None)
	}

	fn install_observed_player_scene(
		&mut self,
		scene_id: SceneId,
		new_game: bool,
		scope: SceneLoadScope,
		sample: Option<&mut SceneSwitchSample<'_>>,
	) -> Result<(), String> {
		let current = self
			.editor
			.play_scene_id()
			.ok_or("Play mode is not active")?;
		if !new_game && current == &scene_id {
			return Ok(());
		}
		self.scene_load_timing = None;
		if new_game {
			self.invalidate_scene_prefetch();
		}
		let mut timing = SceneLoadTiming::new("player", scope, Instant::now());
		if sample.is_some() {
			timing.enable_benchmark();
		}
		let result = self.prepare_player_scene(scene_id, new_game, &mut timing);
		if let Some(sample) = sample {
			sample.observe_install(&timing);
		}
		if result.is_ok() {
			self.scene_load_timing = Some(timing);
		} else {
			timing.finish(SceneLoadOutcome::Failed);
		}
		result
	}

	pub(super) fn exercise_scene_loads(&mut self, samples: u32) -> Result<(), String> {
		if samples == 0 {
			return Ok(());
		}
		let project = self
			.project
			.as_ref()
			.ok_or("Project configuration is unavailable")?;
		let startup = project.startup_scene_id.clone();
		let route = if self.smoke_prefetch_probe {
			gameplay_catalog::cinder_oath::intro_route(project)?
		} else {
			None
		};
		let alternate = route
			.and_then(|(intro, arena)| (intro == startup).then_some(arena))
			.or_else(|| {
				project
					.scenes
					.iter()
					.filter(|scene| {
						scene.id() != &startup
							&& !project
							.world_regions
							.iter()
							.any(|region| &region.scene == scene.id())
					})
					.min_by(|left, right| left.id().as_str().cmp(right.id().as_str()))
					.map(|scene| scene.id().clone())
			})
			.ok_or("Scene load smoke requires two authored full scenes")?;
		for index in 0..samples + 2 {
			let scene = if index % 2 == 0 { &alternate } else { &startup };
			let scope = if index < 2 {
				SceneLoadScope::Warmup
			} else {
				SceneLoadScope::Play
			};
			let source = self
				.editor
				.play_scene_id()
				.cloned()
				.ok_or("Scene load smoke has no source scene")?;
			let before = self.scene_switch_resources()?;
			let frames = self.measure_active_frames(index)?;
			let last_source_frame = Instant::now();
			let mut sample = SceneSwitchSample::new(index, &source, scene, before, frames);
			let result = self
				.install_observed_player_scene(scene.clone(), false, scope, Some(&mut sample))
				.and_then(|()| self.present_scene_switch());
			if result.is_ok() {
				sample.observe_present(
					micros(last_source_frame.elapsed()),
					self.scene_switch_resources()?,
				);
			}
			sample.emit();
			result?;
		}
		self.wait_for_gpu()
	}

	fn scene_switch_resources(&self) -> Result<SceneLoadResources, String> {
		let assets = self
			.asset_manager
			.as_ref()
			.ok_or("Assets are not initialized")?;
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;
		Ok(SceneLoadResources::capture(assets, renderer))
	}

	fn present_scene_switch(&mut self) -> Result<(), String> {
		for _ in 0..8 {
			if self.render()? == renderer::DrawStatus::Rendered {
				return Ok(());
			}
		}
		if let Some(timing) = self.scene_load_timing.take() {
			timing.finish(SceneLoadOutcome::Failed);
		}
		Err("Scene load smoke did not render a frame after installation".into())
	}

	pub(super) fn invalidate_scene_prefetch(&mut self) {
		self.scene_transition = None;
		self.failed_scene_transition = None;
		if let Some(prefetch) = self.scene_prefetch.as_mut() {
			prefetch.invalidate();
		}
	}

	fn measure_active_frames(&mut self, sample: u32) -> Result<[u64; 3], String> {
		let mut frames = [0; 3];
		for (frame, elapsed_us) in frames.iter_mut().enumerate() {
			let started = Instant::now();
			if self.render()? != renderer::DrawStatus::Rendered {
				return Err("Scene switch probe could not draw an active-scene frame".into());
			}
			let elapsed = started.elapsed();
			*elapsed_us = micros(elapsed);
			if self.smoke_prefetch_probe && sample >= 2 && sample % 2 == 0 {
				scene_prefetch::record_active_frame(
					sample,
					frame as u32,
					self.scene_prefetch.is_some(),
					elapsed,
				);
			}
		}
		Ok(frames)
	}

	fn prepare_player_scene(
		&mut self,
		scene_id: SceneId,
		new_game: bool,
		timing: &mut SceneLoadTiming,
	) -> Result<(), String> {
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?;
		let relative_path = project.scene_path(&scene_id)?;
		let asset_root = self
			.asset_root
			.as_ref()
			.ok_or("Asset root is not initialized")?;
		let (scene_path, parsed, outcome, estimated_bytes) =
			timing.measure(BenchmarkPhase::Read, || {
				if let Some(prefetch) = self.scene_prefetch.as_ref()
					&& !new_game
				{
					prefetch.load(&scene_id, Path::new(relative_path))
				} else {
					let (path, parsed) = scene_io::load_confined_data_from_canonical_root(
						asset_root,
						Path::new(relative_path),
					)?;
					Ok((
						path,
						parsed,
						runtime::scene_prefetch::PrefetchOutcome::Disabled,
						0,
					))
				}
			})?;
		if self.scene_prefetch_enabled || self.smoke_prefetch_probe {
			scene_prefetch::record_load(timing, &scene_id, outcome, estimated_bytes);
		}
		let document = Arc::new(parsed.data);
		let prepared = timing.measure(BenchmarkPhase::Simulation, || {
			if new_game {
				self.editor
					.prepare_new_game(scene_id, Arc::clone(&document))
			} else {
				self.editor
					.prepare_play_scene(scene_id, Arc::clone(&document))
			}
		})?;
		self.adopt_prepared_scene(&document, prepared, scene_path, timing)
	}

	pub(super) fn adopt_prepared_scene(
		&mut self,
		document: &runtime::scene_data::SceneData,
		prepared: runtime::scene_editor::PreparedScene,
		scene_path: PathBuf,
		timing: &mut SceneLoadTiming,
	) -> Result<(), String> {
		let scene_id = prepared.scene_id().clone();
		let mix = self
			.feedback_audio
			.prepare_scene_mix(self.player_controls.feedback_mix(), &scene_id)?;
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;
		let assets = self
			.asset_manager
			.as_mut()
			.ok_or("Assets are not initialized")?;
		timing.cpu_ready();
		timing.gpu_started();
		let candidate = timing.measure_native_assembly(assets, renderer, |assets| {
			scene_io::instantiate(document, assets, renderer)
		});

		// Both scenes share one cache and budget until the candidate commits.
		// Completion also makes cache-only allocations from rejected loads safe to prune.
		timing.measure(BenchmarkPhase::Drain, || renderer.wait_idle())?;
		let retain_recent = self.smoke_retain_recent_source && timing.is_benchmark_enabled();
		let installed = timing.measure(BenchmarkPhase::Install, || {
			let installed = candidate.and_then(|(scene, camera)| {
				self.editor.commit_play_scene(
					prepared,
					&mut self.input,
					self.authored.is_some(),
				)?;
				// No fallible operations between committing the CPU world and its view.
				self.scene = scene;
				self.camera = self.editor.play_rig_camera().unwrap_or(camera);
				Ok(())
			});
			if installed.is_ok() && retain_recent {
				assets.prune_after_scene_switch_benchmark();
			} else {
				assets.prune_unused();
			}
			installed
		});
		installed?;
		self.feedback_audio.commit_scene_mix(scene_id, mix);
		self.project_sound.stop_effect();
		self.update_project_sound();
		self.invalidate_scene_prefetch();
		timing.installed();
		self.clear_game_controls();
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
