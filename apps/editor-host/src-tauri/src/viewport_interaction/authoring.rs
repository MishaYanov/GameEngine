use super::*;

impl ViewportInteraction {
	pub(super) fn update_placement(&mut self, normalized: Option<[f64; 2]>, extent: [u32; 2]) {
		let Some(placement) = self.placement.as_mut() else {
			return;
		};
		placement.normalized = normalized;
		placement.position = normalized
			.and_then(|point| {
				self.camera
					.ray(
						[point[0] * extent[0] as f64, point[1] * extent[1] as f64],
						extent,
						self.settings.projection,
					)
					.ok()
			})
			.and_then(|ray| {
				tools::placement_point(
					ray,
					&self.targets,
					self.settings.plane,
					self.settings.snap.then_some(self.settings.move_snap),
				)
			});
	}

	pub fn drain_imports(&mut self, workspace: &mut EditorWorkspace) {
		workspace.drain_asset_jobs();
		if self.import.is_none() {
			return;
		}
		// The UI notifier may have adopted the job first. Completions are retained
		// by the workspace so each consumer can reconcile its own ticket once.
		for completion in workspace.recent_completions() {
			if self.import.as_ref().is_some_and(|ticket| {
				ticket.job_id == completion.job_id
					&& ticket.workspace_generation == completion.workspace_generation
			}) {
				self.import = None;
				self.placement = None;
				self.error = completion.error;
				self.document_changed |= completion.accepted;
				self.changed();
			}
		}
	}

	pub(super) fn cancel_placement(
		&mut self,
		workspace: &mut EditorWorkspace,
	) -> Result<(), String> {
		if let Some(ticket) = self.import.take() {
			if ticket.workspace_generation == workspace.generation {
				let _ = workspace.cancel_import(ticket.job_id, ticket.workspace_generation)?;
			}
		}
		self.placement = None;
		Ok(())
	}

	pub(super) fn cancel_transform(
		&mut self,
		workspace: &mut EditorWorkspace,
	) -> Result<(), String> {
		if self.drag.is_some()
			&& self.drag_generation == Some(workspace.generation)
			&& workspace.backend.session().state().transaction.as_deref() == Some(TRANSFORM_LABEL)
		{
			self.execute(workspace, Command::CancelTransaction)?;
		}
		self.drag = None;
		self.drag_visual = None;
		self.drag_generation = None;
		self.move_targets.clear();
		self.preview = None;
		Ok(())
	}

	pub(super) fn transform_targets(
		&self,
		workspace: &EditorWorkspace,
	) -> Result<Ref<'_, TransformCache>, String> {
		let session = workspace.backend.session();
		let valid = self.transform_cache.borrow().as_ref().is_some_and(|cache| {
			cache.generation == workspace.generation
				&& cache.revision == session.scene_revision()
				&& cache.selection.len() == session.selection().len()
				&& cache
					.selection
					.iter()
					.zip(session.selection())
					.all(|(a, b)| a == b.as_str())
		});
		if valid {
			return Ok(Ref::map(self.transform_cache.borrow(), |cache| {
				cache.as_ref().expect("validated cache")
			}));
		}
		let document = session.authored_document();
		let matrices = document.world_matrices()?;
		let worlds: HashMap<_, _> = document
			.entities
			.iter()
			.zip(matrices.iter())
			.map(|(entity, matrix)| (entity.key.as_str(), *matrix))
			.collect();
		let selected: HashSet<_> = session.selection().iter().map(|key| key.as_str()).collect();
		let parents: HashMap<_, _> = document
			.entities
			.iter()
			.map(|entity| {
				(
					entity.key.as_str(),
					entity.parent.as_ref().map(|parent| parent.as_str()),
				)
			})
			.collect();
		let roots: Vec<_> = document
			.entities
			.iter()
			.zip(matrices)
			.filter(|(entity, _)| {
				if !selected.contains(entity.key.as_str()) {
					return false;
				}
				let mut parent = entity.parent.as_ref().map(|parent| parent.as_str());
				while let Some(key) = parent {
					if selected.contains(key) {
						return false;
					}
					parent = parents.get(key).copied().flatten();
				}
				true
			})
			.collect();
		let mut targets: Vec<_> = roots
			.iter()
			.map(|(entity, world)| TransformTarget {
				key: entity.key.as_str().to_owned(),
				world: *world,
			})
			.collect();
		let move_targets = roots
			.iter()
			.map(|(entity, _)| {
				let parent = entity
					.parent
					.as_ref()
					.and_then(|key| worlds.get(key.as_str()))
					.copied()
					.unwrap_or(Mat4::IDENTITY);
				let determinant = parent.determinant();
				MoveTarget {
					key: entity.key.as_str().to_owned(),
					transform: TransformDto {
						position: entity.transform.position,
						rotation: entity.transform.rotation,
						scale: entity.transform.scale,
					},
					parent_inverse: if determinant.is_finite() && determinant != 0.0 {
						parent.inverse()
					} else {
						Mat4::ZERO
					},
				}
			})
			.collect();
		let order: HashMap<_, _> = session
			.selection()
			.iter()
			.enumerate()
			.map(|(index, key)| (key.as_str(), index))
			.collect();
		targets.sort_by_key(|target| {
			order
				.get(target.key.as_str())
				.copied()
				.unwrap_or(usize::MAX)
		});
		let active_world = session
			.selection()
			.first()
			.and_then(|key| worlds.get(key.as_str()))
			.copied()
			.unwrap_or(Mat4::IDENTITY);
		let active_camera = if session.selection().len() == 1 {
			roots
				.first()
				.and_then(|(entity, _)| entity.camera.as_ref())
				.filter(|camera| camera.enabled)
				.map(|camera| camera.settings.from_world_matrix(active_world))
				.transpose()?
		} else {
			None
		};
		*self.transform_cache.borrow_mut() = Some(TransformCache {
			generation: workspace.generation,
			revision: session.scene_revision(),
			selection: session
				.selection()
				.iter()
				.map(|key| key.as_str().to_owned())
				.collect(),
			targets,
			active_world,
			active_camera,
			move_targets,
		});
		Ok(Ref::map(self.transform_cache.borrow(), |cache| {
			cache.as_ref().expect("initialized cache")
		}))
	}

	pub(super) fn gizmo(
		&self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<Option<Gizmo>, String> {
		if !self.can_author(workspace)
			|| self.settings.tool == ViewportTool::Hand
			|| workspace.backend.session().selection().is_empty()
		{
			return Ok(None);
		}
		// A captured gesture keeps its original basis. Avoid another scene-wide
		// world-matrix/map rebuild after each authored transform preview.
		if self.drag.is_some() {
			return Ok(self.drag_visual.map(|mut gizmo| {
				gizmo.pivot += self.drag_delta;
				gizmo
			}));
		}
		let targets = self.transform_targets(workspace)?;
		if targets.targets.is_empty() {
			return Ok(None);
		}
		let active = targets.active_world;
		let pivot = if self.settings.pivot == ViewportPivot::Center {
			self.selected_bounds(workspace)
				.map(|bounds| {
					(Vec3::from_array(bounds.min()) + Vec3::from_array(bounds.max())) * 0.5
				})
				.unwrap_or(active.w_axis.truncate())
		} else {
			active.w_axis.truncate()
		};
		let rotation = active.to_scale_rotation_translation().1;
		let basis = if self.settings.space == ViewportSpace::Local && rotation.is_finite() {
			rotation
		} else {
			Quat::IDENTITY
		};
		Ok(Some(Gizmo::new(
			pivot,
			basis,
			self.camera
				.gizmo_size(pivot, extent, self.settings.projection, 80.0 * dpi as f32),
		)))
	}

	pub(super) fn select_click(
		&mut self,
		workspace: &mut EditorWorkspace,
		click: Click,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<(), String> {
		let dx = click.current[0] - click.start[0];
		let dy = click.current[1] - click.start[1];
		let hits: Vec<String> = if dx.hypot(dy) > 4.0 * dpi {
			let matrix = self
				.camera
				.view_projection(extent, self.settings.projection)?;
			let min = Vec2::new(
				click.start[0].min(click.current[0]) as f32,
				click.start[1].min(click.current[1]) as f32,
			);
			let max = Vec2::new(
				click.start[0].max(click.current[0]) as f32,
				click.start[1].max(click.current[1]) as f32,
			);
			self.targets
				.iter()
				.filter(|target| {
					let mut bmin = Vec2::splat(f32::INFINITY);
					let mut bmax = Vec2::splat(f32::NEG_INFINITY);
					let mut visible = false;
					for point in bounds_corners(target.bounds) {
						if let Some(projected) = tools::project(matrix, extent, point) {
							bmin = bmin.min(projected);
							bmax = bmax.max(projected);
							visible = true;
						}
					}
					if !visible {
						return false;
					}
					if dx >= 0.0 {
						(bmin.cmpge(min) & bmax.cmple(max)).all()
					} else {
						(bmax.cmpge(min) & bmin.cmple(max)).all()
					}
				})
				.take(4096)
				.map(|target| target.key.clone())
				.collect()
		} else {
			tools::pick_nearest(
				self.camera
					.ray(click.current, extent, self.settings.projection)?,
				&self.targets,
			)
			.map(|hit| vec![hit.key])
			.unwrap_or_default()
		};
		let mut keys = if click.additive || click.toggle {
			click.selection
		} else {
			Vec::new()
		};
		for hit in hits {
			if click.toggle && keys.contains(&hit) {
				keys.retain(|key| key != &hit);
			} else if !keys.contains(&hit) && keys.len() < 4096 {
				keys.push(hit);
			}
		}
		self.execute(workspace, Command::Select { keys })?;
		Ok(())
	}
}
