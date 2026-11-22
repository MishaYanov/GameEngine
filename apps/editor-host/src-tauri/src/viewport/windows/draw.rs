use super::*;

impl Worker {
	pub(super) fn draw(&mut self) -> Result<(), String> {
		let result = self.draw_frame();
		if result.is_err() {
			if let Some((_, timing)) = self.authored_load.take() {
				timing.finish(LoadOutcome::Failed);
			}
			if let Some((_, _, _, timing)) = self.play_load.take() {
				timing.finish(LoadOutcome::Failed);
			}
		}
		result
	}

	fn draw_frame(&mut self) -> Result<(), String> {
		if !self.active() {
			return Ok(());
		}
		let measurement_started = self.warm_measurement.then(Instant::now);
		let was_captured = self.interaction.captured();
		let packet = loop {
			let captured = {
				let mut workspace = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let Some(workspace) = workspace.as_mut() else {
					self.authored_load = None;
					self.play_load = None;
					return Ok(());
				};
				self.interaction.sync(workspace)?;
				if self
					.gpu
					.as_ref()
					.is_some_and(|gpu| gpu.asset_root != workspace.asset_root)
				{
					None
				} else {
					let session = workspace.backend.session();
					let revision = session.scene_revision();
					let generation = workspace.generation;
					let cursor = workspace.backend.change_cursor();
					let mut synchronized = false;
					let mut lights_changed = false;
					// Only CPU metadata/matrices are touched under the workspace
					// lock. Descriptor uploads, waits, acquisition and presentation
					// happen after this packet has released the lock.
					if let Some(gpu) = self.gpu.as_mut() {
						if gpu.asset_root == workspace.asset_root
							&& gpu.generation == generation
							&& !gpu.assets_invalidated
						{
							if gpu.revision == revision {
								synchronized = true;
							} else {
								let page = workspace
									.backend
									.changes_since(gpu.change_cursor)
									.map_err(|error| error.to_string())?;
								let mut changes = runtime::world::WorldChange {
									full_resync: page.resync_required,
									..runtime::world::WorldChange::default()
								};
								for event in page.events {
									match event.event {
										runtime::events::BackendEventKind::DocumentChanged {
											changes: delta,
										}
										| runtime::events::BackendEventKind::SimulationAdvanced {
											changes: delta,
											..
										} => changes.merge(delta),
										runtime::events::BackendEventKind::RuntimeSceneChanged { .. } => {
											changes.full_resync = true;
										}
										runtime::events::BackendEventKind::StateChanged
										| runtime::events::BackendEventKind::ProjectChanged => {}
									}
								}
								if changes.is_empty() {
									changes.full_resync = true;
								}
								synchronized = if let Some(world) = session.render_world() {
									{
										if changes.topology_changed
											|| !changes.removed_entities.is_empty()
											|| changes.full_resync
										{
											gpu.renderer.wait_idle()?;
										}
										let synchronized =
											gpu.scene.synchronize_world(world, &changes)?;
										if synchronized
											&& (changes.topology_changed
												|| !changes.removed_entities.is_empty())
										{
											gpu.assets.prune_unused();
										}
										synchronized
									}
								} else {
									gpu.scene.synchronize_authored(
										session.validated_authored_document(),
										&changes,
									)?
								};
								lights_changed = changes.lights_changed || changes.full_resync;
							}
							if synchronized {
								gpu.revision = revision;
								gpu.change_cursor = cursor;
							}
						}
					}
					let document = if synchronized {
						None
					} else {
						Some(session.render_document())
					};
					let game_camera = self.interaction.game_camera(workspace);
					let scene_id = (!session.is_playing()
						&& self.interaction.settings.view == bridge::viewport::ViewportView::Scene)
						.then(|| session.active_scene_id().clone());
					let warm_requested =
						self.warm_measurement && workspace.scene_warming.is_requested();
					let warmed = match (&scene_id, &document) {
						(Some(scene), Some(document)) => {
							workspace.scene_warming.claim(generation, scene, document)
						}
						_ => crate::editor::scene_warming::WarmClaim::Unrequested,
					};
					Some(RenderPacket {
						warm_requested,
						warmed,
						animations: session.animations().cloned(),
						animation_preview: workspace.active_animation_preview(),
						document,
						game_camera,
						revision,
						generation,
						scene_id,
						switch_clock: workspace.scene_switch,
						change_cursor: cursor,
						asset_root: workspace.asset_root.clone(),
						lights_changed,
					})
				}
			};
			if let Some(packet) = captured {
				break packet;
			}
			// Source-root retirement can wait on the GPU. No workspace
			// lock or strong document snapshot survives into that wait.
			self.gpu = None;
			self.prepared_extent = None;
		};
		self.release_ended_scene_capture(was_captured);
		let RenderPacket {
			warmed,
			warm_requested,
			animations,
			animation_preview,
			mut document,
			game_camera,
			revision,
			generation,
			scene_id,
			switch_clock,
			change_cursor,
			asset_root,
			lights_changed,
		} = packet;
		let key = scene_id.as_ref().map(|scene_id| AuthoredLoadKey {
			generation,
			revision,
			scene_id: scene_id.clone(),
		});
		let previous = key.as_ref().and_then(|key| {
			self.status.lock().ok().and_then(|status| {
				status.scene_load.as_ref().and_then(|progress| {
					(progress.workspace_generation == key.generation
						&& progress.scene_id == key.scene_id
						&& switch_clock
						.is_none_or(|(accepted, _, _)| progress.scene_revision >= accepted))
						.then_some(progress.stage)
				})
			})
		});
		let loading = key.is_some() && previous != Some(ViewportSceneLoadStage::Presented);
		if !loading
			|| self
			.authored_load
			.as_ref()
			.is_some_and(|(active, _)| Some(active) != key.as_ref())
		{
			self.authored_load = None;
		}
		let new_load = loading && self.authored_load.is_none();
		if let Some(key) = key.as_ref().filter(|_| new_load) {
			let now = Instant::now();
			let (_, requested, ready) = switch_clock.unwrap_or((revision, now, now));
			let mut timing = SceneLoadTiming::new("editor", LoadScope::Authored, requested);
			timing.cpu_ready_at(ready);
			self.authored_load = Some((key.clone(), timing));
		}
		let request_id = self
			.authored_load
			.as_ref()
			.map(|(_, timing)| timing.id().to_string());
		let (mut warm_references, warm_outcome) = warmed.into_references();
		if self.warm_measurement && new_load {
			if let (Some(request_id), Some(scene)) = (&request_id, &scene_id) {
				log_warm(serde_json::json!({
					"schema_version": 1, "kind": "load", "request_id": request_id,
					"scene_id": scene.as_str(), "outcome": warm_outcome,
				}));
			}
		}
		let (parent, control, status, shared) =
			(&self.parent, &self.control, &self.status, &self.shared);
		let is_current = || {
			key.as_ref().is_none_or(|key| {
				shared.lock().ok().is_some_and(|slot| {
					slot.as_ref()
						.is_some_and(|workspace| key.matches(workspace))
				})
			})
		};
		let report = |stage, count: Option<(usize, usize)>| {
			if let (Some(key), Some(request_id)) = (key.as_ref(), request_id.as_ref()) {
				let Ok(workspace) = shared.lock() else { return };
				if !workspace
					.as_ref()
					.is_some_and(|workspace| key.matches(workspace))
				{
					return;
				}
				let (completed, total) = count.unzip();
				publish_scene_load(
					parent,
					control,
					status,
					ViewportSceneLoadDto {
						request_id: request_id.clone(),
						workspace_generation: key.generation,
						scene_id: key.scene_id.clone(),
						scene_revision: key.revision,
						stage,
						completed,
						total,
					},
				);
			}
		};
		if new_load {
			report(ViewportSceneLoadStage::Preparing, None);
		}
		if let Some((_, timing)) = self.authored_load.as_mut() {
			timing.gpu_started();
		}
		let mut last_count = None;
		let mut count_prepared = |completed, total| {
			if !loading {
				return;
			}
			let now = Instant::now();
			if completed == total
				|| last_count.is_none_or(|last| now.duration_since(last) >= SCENE_LOAD_INTERVAL)
			{
				report(ViewportSceneLoadStage::Preparing, Some((completed, total)));
				last_count = Some(now);
			}
		};
		if self.gpu.is_none() {
			let window = Arc::clone(
				self.window
					.as_ref()
					.ok_or("Native viewport window is not initialized")?,
			);
			let [width, height] = self.extent();
			let mut renderer = WindowRenderer::new(Arc::clone(&window), width, height)?;
			let mut assets = AssetManager::new(&asset_root).map_err(|error| error.to_string())?;
			let data = document
				.take()
				.ok_or("Initial viewport packet is missing scene data")?;
			let candidate = scene_io::instantiate_with_references(
				&data,
				&mut assets,
				&renderer,
				warm_references.take(),
				&mut count_prepared,
				|| !is_current(),
			)?;
			renderer.wait_idle()?;
			let install_guard = shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let current = key.as_ref().is_none_or(|key| {
				install_guard
					.as_ref()
					.is_some_and(|workspace| key.matches(workspace))
			});
			let Some((scene, _)) = candidate.filter(|_| current) else {
				self.authored_load = None;
				self.dirty = true;
				return Ok(());
			};
			renderer.set_lights(&scene.renderer_lights())?;
			engine_logger::debug!(target: "editor_viewport", "Viewport GPU state created: generation={generation}, width={width}, height={height}, entities={}, lights={}", data.entities.len(), data.lights.len());
			drop(data);
			self.gpu = Some(GpuState {
				scene,
				assets: ManuallyDrop::new(assets),
				renderer,
				asset_root,
				revision,
				generation,
				change_cursor,
				assets_invalidated: false,
			});
			drop(install_guard);
		}
		let gpu = self
			.gpu
			.as_mut()
			.ok_or("Viewport GPU state is not initialized")?;
		let generation_changed = gpu.generation != generation;
		if gpu.revision != revision || generation_changed || gpu.assets_invalidated {
			let data = document
				.take()
				.ok_or("Full viewport synchronization packet is missing scene data")?;
			let replacement =
				if loading || gpu.assets_invalidated || !gpu.scene.synchronize(&data)? {
					let (scene, _) = match scene_io::instantiate_with_references(
						&data,
						&mut gpu.assets,
						&gpu.renderer,
						warm_references.take(),
						&mut count_prepared,
						|| !is_current(),
					) {
						Ok(Some(candidate)) => candidate,
						Ok(None) => {
							gpu.renderer.wait_idle()?;
							gpu.assets.prune_unused();
							self.authored_load = None;
							self.dirty = true;
							return Ok(());
						}
						Err(error) => {
							drop(data);
							gpu.renderer.wait_idle().map_err(|wait_error| {
								format!(
								"{error}; failed to retire rejected candidate GPU resources: {wait_error}"
							)
							})?;
							gpu.assets.prune_unused();
							return Err(error);
						}
					};
					Some(scene)
				} else {
					None
				};
			drop(data);
			if let Some(scene) = replacement {
				gpu.renderer.wait_idle()?;
				let install_guard = shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				if key.as_ref().is_some_and(|key| {
					!install_guard
						.as_ref()
						.is_some_and(|workspace| key.matches(workspace))
				}) {
					drop(install_guard);
					drop(scene);
					gpu.assets.prune_unused();
					self.authored_load = None;
					self.dirty = true;
					return Ok(());
				}
				let retired_scene = std::mem::replace(&mut gpu.scene, scene);
				drop(install_guard);
				drop(retired_scene);
				gpu.assets.prune_unused();
			} else if generation_changed {
				gpu.renderer.wait_idle()?;
				gpu.assets.prune_unused();
			}
			gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
			gpu.revision = revision;
			gpu.generation = generation;
			gpu.change_cursor = change_cursor;
			gpu.assets_invalidated = false;
		} else if lights_changed {
			gpu.renderer.set_lights(&gpu.scene.renderer_lights())?;
		}
		if !is_current() {
			self.authored_load = None;
			self.dirty = true;
			return Ok(());
		}
		if let Some((_, timing)) = self.authored_load.as_mut() {
			timing.installed();
		}
		let play_is_current = |expected_generation: u64, run_id: &str, scene_generation: u64| {
			shared.lock().ok().is_some_and(|slot| {
				slot.as_ref().is_some_and(|workspace| {
					workspace.generation == expected_generation
						&& workspace
						.backend
						.session()
						.owns_play_scene(run_id, scene_generation)
				})
			})
		};
		if self
			.play_load
			.as_ref()
			.is_some_and(|(generation, run_id, scene_generation, _)| {
				!play_is_current(*generation, run_id, *scene_generation)
			}) {
			self.play_load = None;
		}
		gpu.scene.reset_animation_overrides();
		if let Some(bank) = &animations {
			gpu.scene.synchronize_animation(bank)?;
		} else {
			gpu.scene.restore_authored_animation();
		}
		if game_camera.is_none() {
			if let Some((key, pose)) = &animation_preview {
				gpu.scene.set_animation_pose(key, Some(pose))?;
			}
		}
		// Do not hold a strong document snapshot across frame fences or
		// presentation. Play can mutate its uniquely owned Arc in place.
		drop(document);
		if loading {
			report(ViewportSceneLoadStage::Rendering, None);
		}
		let Some([width, height]) = gpu.renderer.prepare()? else {
			self.prepared_extent = None;
			if let Ok(mut status) = self.status.lock() {
				status.width = 0;
				status.height = 0;
			}
			return Ok(());
		};
		if self.prepared_extent != Some([width, height]) {
			self.last_status = Instant::now() - Duration::from_secs(1);
		}
		self.prepared_extent = Some([width, height]);
		let (uniform, game_rect) = if let Some(camera) = game_camera {
			let rect = GameContentRect::fit(width, height).ok_or("Invalid Game viewport extent")?;
			(
				CameraUniform::from_view_projection(camera.view_projection(rect.aspect())?),
				Some(rect),
			)
		} else {
			self.interaction.synchronize_targets(
				&gpu.scene,
				generation,
				revision,
				[width, height],
			)?;
			(
				CameraUniform::from_view_projection(
					self.interaction
						.camera
						.view_projection([width, height], self.interaction.settings.projection)?,
				),
				None,
			)
		};
		let visuals = if game_rect.is_some() {
			Default::default()
		} else {
			let workspace = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			self.interaction.visuals(
				workspace.as_ref().ok_or("No scene is open")?,
				[width, height],
				self.dpi,
			)?
		};
		gpu.renderer.set_editor_visuals(&visuals)?;
		let frustum = runtime::visibility::Frustum::from_view_projection(
			glam::Mat4::from_cols_array(&uniform.view_projection),
		)?;
		let (objects, _) = gpu.scene.try_render_visible(Some(&frustum))?;
		let frame_status = match game_rect {
			Some(rect) => gpu
				.renderer
				.draw_in_rect(&uniform, &objects, rect.pixels())?,
			None => gpu.renderer.draw(&uniform, &objects)?,
		};
		let presented_at = (frame_status == DrawStatus::Rendered).then(Instant::now);
		if !loading {
			if let (Some(when), Some(started), Some(scene)) =
				(presented_at, measurement_started, &scene_id)
			{
				log_warm(serde_json::json!({
					"schema_version": 1, "kind": "active_frame", "scene_id": scene.as_str(),
					"warming_requested": warm_requested,
					"elapsed_us": when.saturating_duration_since(started).as_micros().min(u64::MAX as u128) as u64,
				}));
			}
		}
		match frame_status {
			DrawStatus::NeedsRedraw => {
				self.dirty = true;
				self.redraw_retry = Some(Instant::now() + FRAME);
			}
			DrawStatus::Rendered | DrawStatus::Suspended => {
				self.redraw_retry = None;
			}
		}
		let metrics = gpu.renderer.last_metrics();
		if let Ok(mut status) = self.status.lock() {
			if status.attached {
				status.width = width;
				status.height = height;
				status.draw_calls = metrics.draw_calls as usize;
				status.instances = metrics.instances as usize;
				status.triangles = metrics.triangles as usize;
				status.gpu_time_ms = metrics.gpu_time_ms;
				status.gpu_timestamps_supported = metrics.gpu_timestamps_supported;
				if !loading && frame_status == DrawStatus::Rendered {
					self.status_pending |= advance_presented_revision(
						&mut status,
						generation,
						scene_id.as_ref(),
						revision,
					);
				}
			}
		}
		if let Some(when) = presented_at {
			if let Some((_, timing)) = self.authored_load.take() {
				if is_current() {
					report(ViewportSceneLoadStage::Presented, None);
					timing.finish_at(LoadOutcome::Presented, when);
				}
			}
			if let Some((generation, run_id, scene_generation, timing)) = self.play_load.take() {
				if play_is_current(generation, &run_id, scene_generation) {
					timing.finish_at(LoadOutcome::Presented, when);
				}
			}
		}
		self.publish_editor(false);
		Ok(())
	}
}

fn log_warm(message: serde_json::Value) {
	engine_logger::info!(target: "editor_scene_warming", "{message}");
}
