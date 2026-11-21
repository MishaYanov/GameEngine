use super::*;

impl Worker {
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
			let transition = outcome.requested_scene.map(|target| {
				(
					workspace.generation,
					target,
					workspace.backend.prepare_pending_play_scene(),
				)
			});
			(transition, outcome.pending_steps)
		};

		if let Some((generation, target, prepared)) = transition {
			self.last_tick = None;
			let result = prepared
				.map_err(|error| error.to_string())
				.and_then(|prepared| {
					self.install_play_scene(
						PlayInstall::Transition {
							generation,
							target: &target,
						},
						prepared,
					)
				});
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

	fn install_play_scene(
		&mut self,
		guard: PlayInstall<'_>,
		(prepared, document): (PreparedScene, Arc<runtime::SceneData>),
	) -> Result<(), String> {
		let gpu = self
			.gpu
			.as_mut()
			.ok_or("Native viewport renderer is not initialized")?;
		let candidate = scene_io::instantiate(&document, &mut gpu.assets, &gpu.renderer);
		drop(document);
		gpu.renderer.wait_idle()?;
		let installed = candidate.and_then(|(scene, _)| {
			let mut shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let Some(workspace) = shared.as_mut() else {
				return match guard {
					PlayInstall::Transition { .. } => Ok(()),
					PlayInstall::Restart(_) => Err("The project was closed during restart".into()),
				};
			};
			if !guard.validate(workspace, Instant::now())? {
				return Ok(());
			}

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
			drop(shared);
			drop(retired_scene);
			Ok(())
		});
		// Candidate-only resources are also pruned after a stale or failed commit.
		gpu.assets.prune_unused();
		installed
	}

	pub(super) fn restart_run(&mut self, request: &ActionRequest) -> Result<(), String> {
		let prepared = {
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
				.map_err(|error| error.to_string())?
		};
		self.install_play_scene(PlayInstall::Restart(request), prepared)
	}
}
