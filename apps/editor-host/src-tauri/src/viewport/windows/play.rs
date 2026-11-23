use super::*;

impl Worker {
	pub(super) fn sync_scene_audio(&mut self) {
		use runtime::scene_audio::{SceneAudioMix, SceneAudioOwner};

		let snapshot = self.shared.lock().ok().and_then(|guard| {
			let workspace = guard.as_ref()?;
			let status = workspace.backend.play_status()?;
			let scene = workspace.backend.session().render_document();
			Some((status, scene, workspace.asset_root.clone()))
		});
		let audible = self.active()
			&& !self.failed
			&& self.interaction.settings.view == bridge::viewport::ViewportView::Game
			&& snapshot
				.as_ref()
				.is_some_and(|(status, _, _)| !status.paused);
		let next = snapshot.as_ref().map(|(status, scene, path)| {
			(
				SceneAudioOwner::from_play(status),
				scene.as_ref(),
				path.as_path(),
			)
		});
		let shared = &self.shared;
		self.scene_audio
			.synchronize(next, audible, SceneAudioMix::default(), |owner| {
				shared.lock().ok().is_some_and(|guard| {
					guard
						.as_ref()
						.and_then(|workspace| workspace.backend.play_status())
						.is_some_and(|status| {
							status.run_id == owner.run_id
								&& status.scene_generation == owner.scene_generation
						})
				})
			});
	}

	pub(super) fn poll_gamepad(&mut self, now: Instant) -> Option<Instant> {
		let context = if self.active() && !self.failed && self.native_keyboard_focused() {
			self.shared
				.lock()
				.ok()
				.and_then(|guard| self.interaction.gamepad_context(guard.as_ref()?))
		} else {
			None
		};
		let Some(context) = context else {
			self.interaction.reset_gamepad();
			return None;
		};
		// XInput can query multiple slots on discovery. Never do that while
		// holding the workspace lock used by document and simulation work.
		if self.interaction.poll_gamepad(now) {
			let result = self.shared.lock().ok().and_then(|mut guard| {
				let workspace = guard.as_mut()?;
				let current = self.interaction.gamepad_context(workspace)?;
				if current != context {
					self.interaction.reset_gamepad();
				}
				let result = self.interaction.route_game_input(workspace);
				if result.is_err() {
					self.interaction.clear_play_input(workspace);
				}
				Some(result)
			});
			match result {
				Some(Ok(())) => {}
				Some(Err(error)) => self
					.interaction
					.report(format!("Controller input was rejected: {error}")),
				None => self.interaction.reset_gamepad(),
			}
		}
		self.interaction.gamepad_deadline().or(Some(now + FRAME))
	}

	pub(super) fn clear_play_input(&mut self) {
		if let Ok(mut workspace) = self.shared.lock() {
			if let Some(workspace) = workspace.as_mut() {
				self.interaction.clear_play_input(workspace);
			}
		}
		self.last_tick = None;
	}

	pub(super) fn tick_play(&mut self, elapsed: f64) -> Result<(), String> {
		let (transition, pending_steps) = {
			let mut workspace = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let workspace = workspace.as_mut().ok_or("No editor document is open")?;
			if !self.gpu_matches(workspace) {
				self.last_tick = None;
				self.dirty = true;
				return Ok(());
			}
			if !workspace.backend.has_play_visibility_capabilities() {
				let gpu = self.gpu.as_ref().expect("matching GPU scene checked above");
				if gpu.revision != workspace.backend.session().scene_revision()
					|| gpu.assets_invalidated
				{
					self.last_tick = None;
					self.dirty = true;
					return Ok(());
				}
				workspace
					.backend
					.set_play_visibility_capabilities(gpu.scene.visibility_capabilities())?;
			}
			let outcome = workspace.backend.tick(elapsed);
			self.interaction
				.sync_game_status_after_tick(workspace, &outcome);
			let outcome = outcome.map_err(|error| error.to_string())?;
			self.dirty |= !outcome.changes.is_empty();
			let transition = outcome.requested_scene.and_then(|target| {
				let status = workspace.backend.play_status()?;
				let timing = SceneLoadTiming::new("editor", LoadScope::Play, Instant::now());
				Some((
					workspace.generation,
					target,
					status.run_id,
					status.scene_generation,
					workspace.backend.prepare_pending_play_scene(),
					timing,
				))
			});
			(transition, outcome.pending_steps)
		};

		if let Some((generation, target, run_id, scene_generation, prepared, timing)) = transition {
			self.last_tick = None;
			let result = self.install_timed_play_scene(
				PlayInstall::Transition {
					generation,
					target: &target,
					run_id: &run_id,
					scene_generation,
				},
				prepared.map_err(|error| error.to_string()),
				timing,
			);
			if let Err(error) = result {
				let current = self.shared.lock().ok().is_some_and(|workspace| {
					workspace.as_ref().is_some_and(|workspace| {
						workspace.generation == generation
							&& workspace.backend.pending_play_scene() == Some(&target)
					})
				});
				if current {
					engine_logger::error!(target: "editor_viewport", "Play scene transition failed: operation=install_play_scene");
					self.interaction.report(format!(
						"Could not switch to game scene {:?}: {error}",
						target.as_str()
					));
				}
			}
		}
		self.fulfill_runtime_spawns()?;
		if pending_steps > 0 {
			self.tick_play(0.0)?;
		}
		Ok(())
	}

	fn fulfill_runtime_spawns(&mut self) -> Result<(), String> {
		let (generation, run_id, root, requests) = {
			let shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let workspace = shared.as_ref().ok_or("No editor document is open")?;
			if workspace.backend.session().is_paused() {
				return Ok(());
			}
			let Some(status) = workspace.backend.play_status() else {
				return Ok(());
			};
			(
				workspace.generation,
				status.run_id,
				workspace.asset_root.clone(),
				workspace.backend.pending_runtime_spawns(),
			)
		};
		for request in requests {
			let result = (|| {
				let (prefab, fingerprint) =
					bridge::runtime_prefab::load(&root, &request.prefab_path)?;
				let (mut prepared, data) = {
					let shared = self
						.shared
						.lock()
						.map_err(|_| "Editor document lock is poisoned")?;
					let workspace = shared.as_ref().ok_or("Project closed during spawn")?;
					if workspace.generation != generation
						|| workspace
							.backend
							.play_status()
							.is_none_or(|status| status.run_id != run_id)
					{
						return Err("Run changed during spawn".into());
					}
					let prepared = workspace.backend.prepare_runtime_spawn(
						&prefab,
						&fingerprint,
						request.position,
						Some(request.id),
					)?;
					let data = workspace.backend.runtime_spawn_document(&prepared)?;
					(prepared, data)
				};
				let gpu = self.gpu.as_mut().ok_or("Native viewport is unavailable")?;
				let candidate = scene_io::instantiate(&data, &mut gpu.assets, &gpu.renderer);
				gpu.renderer.wait_idle()?;
				let (mut scene, _) = candidate?;
				let mut shared = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let workspace = shared.as_mut().ok_or("Project closed during spawn")?;
				if workspace.generation != generation
					|| workspace
						.backend
						.play_status()
						.is_none_or(|status| status.run_id != run_id)
				{
					return Err("Run changed during spawn".into());
				}
				// Recheck the topology token and include transforms from any accepted tick.
				let current = workspace.backend.runtime_spawn_document(&prepared)?;
				if !scene.synchronize(&current)? {
					return Err("Spawn candidate topology changed".into());
				}
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
				workspace
					.backend
					.commit_runtime_spawn(prepared)
					.map_err(|error| error.to_string())?;
				let retired = std::mem::replace(&mut gpu.scene, scene);
				gpu.revision = workspace.backend.session().scene_revision();
				gpu.change_cursor = workspace.backend.change_cursor();
				self.dirty = true;
				drop(shared);
				drop(retired);
				Ok::<(), String>(())
			})();
			if let Err(error) = result {
				let mut shared = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				if let Some(workspace) = shared.as_mut() {
					if workspace.generation == generation
						&& workspace
							.backend
							.play_status()
							.is_some_and(|status| status.run_id == run_id)
					{
						workspace
							.backend
							.reject_runtime_spawn(request.id, &error)
							.map_err(|error| error.to_string())?;
						engine_logger::error!(target:"editor_viewport", "Runtime spawn rejected: operation=prepare_and_admit_runtime_spawn");
					}
				}
			}
			if let Some(gpu) = self.gpu.as_mut() {
				gpu.assets.prune_unused();
			}
		}
		Ok(())
	}

	pub(super) fn gpu_matches(&self, workspace: &EditorWorkspace) -> bool {
		self.gpu.as_ref().is_some_and(|gpu| {
			gpu.generation == workspace.generation && gpu.asset_root == workspace.asset_root
		})
	}

	fn install_timed_play_scene(
		&mut self,
		guard: PlayInstall<'_>,
		prepared: Result<(PreparedScene, Arc<runtime::SceneData>), String>,
		mut timing: SceneLoadTiming,
	) -> Result<(), String> {
		self.play_load = None;
		let result = prepared.and_then(|data| self.install_play_scene(guard, data, &mut timing));
		match result {
			Ok(Some((generation, run_id, scene_generation))) => {
				self.play_load = Some((generation, run_id, scene_generation, timing));
				Ok(())
			}
			Ok(None) => Ok(()),
			Err(error) => {
				timing.finish(LoadOutcome::Failed);
				Err(error)
			}
		}
	}

	fn install_play_scene(
		&mut self,
		guard: PlayInstall<'_>,
		(prepared, document): (PreparedScene, Arc<runtime::SceneData>),
		timing: &mut SceneLoadTiming,
	) -> Result<Option<(u64, String, u64)>, String> {
		let gpu = self
			.gpu
			.as_mut()
			.ok_or("Native viewport renderer is not initialized")?;
		timing.cpu_ready();
		timing.gpu_started();
		let candidate = scene_io::instantiate_cancellable(
			&document,
			&mut gpu.assets,
			&gpu.renderer,
			|_, _| {},
			|| {
				!self.shared.lock().ok().is_some_and(|slot| {
					slot.as_ref().is_some_and(|workspace| {
						guard.validate(workspace, Instant::now()) == Ok(true)
					})
				})
			},
		);
		drop(document);
		gpu.renderer.wait_idle()?;
		let installed = candidate.and_then(|candidate| {
			let mut shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let Some(workspace) = shared.as_mut() else {
				return match guard {
					PlayInstall::Transition { .. } => Ok(None),
					PlayInstall::Restart(_) => Err("The project was closed during restart".into()),
				};
			};
			if !guard.validate(workspace, Instant::now())? {
				return Ok(None);
			}
			let Some((scene, _)) = candidate else {
				return match guard {
					PlayInstall::Transition { .. } => Ok(None),
					PlayInstall::Restart(_) => {
						Err("Restart was cancelled during preparation".into())
					}
				};
			};

			// Lights are validated CPU data. Keep renderer uploads and waits
			// outside the workspace lock and restore the old lights on rejection.
			gpu.renderer.set_lights(&scene.renderer_lights())?;
			if let Err(error) = workspace.backend.commit_play_scene(prepared) {
				gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
				return Err(error.to_string());
			}
			let retired_scene = std::mem::replace(&mut gpu.scene, scene);
			gpu.revision = workspace.backend.session().scene_revision();
			gpu.change_cursor = workspace.backend.change_cursor();
			self.interaction.clear_play_input(workspace);
			self.interaction.clear_error();
			self.interaction.sync_game_status(workspace);
			self.last_tick = None;
			self.dirty = true;
			let presented_key = workspace
				.backend
				.play_status()
				.map(|status| (workspace.generation, status.run_id, status.scene_generation));
			timing.installed();
			drop(shared);
			drop(retired_scene);
			Ok(presented_key)
		});
		// Candidate-only resources are also pruned after a stale or failed commit.
		gpu.assets.prune_unused();
		installed
	}

	pub(super) fn restart_run(&mut self, request: &ActionRequest) -> Result<(), String> {
		let timing = SceneLoadTiming::new("editor", LoadScope::Play, Instant::now());
		let prepared = (|| {
			let shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let workspace = shared
				.as_ref()
				.ok_or("Open a project before restarting a run")?;
			PlayInstall::Restart(request).validate(workspace, Instant::now())?;
			if !self.gpu_matches(workspace) {
				return Err(
					"Wait for the current project to finish loading before restarting".into(),
				);
			}
			let ViewportActionDto::RestartRun {
				run_id,
				expected_revision,
			} = &request.action
			else {
				return Err("Expected a Restart Run request".into());
			};
			workspace
				.backend
				.prepare_restart_play(run_id, *expected_revision)
				.map_err(|error| error.to_string())
		})();
		self.install_timed_play_scene(PlayInstall::Restart(request), prepared, timing)
	}
}
