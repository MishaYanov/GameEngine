use super::*;

impl Worker {
	pub(super) fn draw(&mut self) -> Result<(), String> {
		if !self.active() {
			return Ok(());
		}
		let was_captured = self.interaction.captured();
		let packet = loop {
			let captured = {
				let mut workspace = self
					.shared
					.lock()
					.map_err(|_| "Editor document lock is poisoned")?;
				let workspace = workspace
					.as_mut()
					.ok_or("Open a scene before attaching a viewport")?;
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
					Some(RenderPacket {
						animations: session.animations().cloned(),
						animation_preview: workspace.active_animation_preview(),
						document,
						game_camera,
						revision,
						generation,
						scene_id: (!session.is_playing()
							&& self.interaction.settings.view
								== bridge::viewport::ViewportView::Scene)
							.then(|| session.active_scene_id().clone()),
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
			animations,
			animation_preview,
			mut document,
			game_camera,
			revision,
			generation,
			scene_id,
			change_cursor,
			asset_root,
			lights_changed,
		} = packet;
		let previous = scene_id.as_ref().and_then(|scene_id| {
			self.status.lock().ok().and_then(|status| {
				status.scene_load.as_ref().and_then(|progress| {
					(progress.workspace_generation == generation && &progress.scene_id == scene_id)
						.then_some(progress.stage)
				})
			})
		});
		let loading = scene_id.is_some() && previous != Some(ViewportSceneLoadStage::Presented);
		let (parent, control, status) = (&self.parent, &self.control, &self.status);
		let report = |stage, count: Option<(usize, usize)>| {
			if let Some(scene_id) = scene_id.as_ref() {
				let (completed, total) = count.unzip();
				publish_scene_load(
					parent,
					control,
					status,
					ViewportSceneLoadDto {
						workspace_generation: generation,
						scene_id: scene_id.clone(),
						scene_revision: revision,
						stage,
						completed,
						total,
					},
				);
			}
		};
		if loading && previous.is_none() {
			report(ViewportSceneLoadStage::Preparing, None);
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
			let (scene, _) = scene_io::instantiate_with_progress(
				&data,
				&mut assets,
				&renderer,
				&mut count_prepared,
			)?;
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
			let replacement = if gpu.assets_invalidated || !gpu.scene.synchronize(&data)? {
				let (scene, _) = match scene_io::instantiate_with_progress(
					&data,
					&mut gpu.assets,
					&gpu.renderer,
					&mut count_prepared,
				) {
					Ok(candidate) => candidate,
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
				gpu.scene = scene;
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
		if loading && frame_status == DrawStatus::Rendered {
			report(ViewportSceneLoadStage::Presented, None);
		}
		self.publish_editor(false);
		Ok(())
	}
}
