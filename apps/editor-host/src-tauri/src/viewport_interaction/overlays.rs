use super::*;

impl ViewportInteraction {
	pub fn visuals(
		&self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
		dpi: f64,
	) -> Result<EditorVisuals, String> {
		if self.camera_preview_active || self.settings.view == ViewportView::Game {
			return Ok(EditorVisuals::default());
		}
		let mut visuals = EditorVisuals::default();
		if self.settings.grid {
			let (normal, tangent, _) = tools::plane_basis(self.settings.plane);
			visuals.grid = Some(EditorGrid {
				origin: [0.0; 3],
				normal: normal.to_array(),
				tangent: tangent.to_array(),
				spacing: tools::adaptive_grid_spacing(
					self.settings.grid_spacing,
					self.camera.units_per_pixel(
						self.camera.pivot,
						extent,
						self.settings.projection,
					),
				),
				opacity: 0.38,
				fade_distance: (self.camera.distance * 15.0).clamp(100.0, 100_000.0),
			});
		}
		let selection = workspace.backend.session().selection();
		if let Some(index) = self
			.hovered
			.as_ref()
			.and_then(|key| self.target_indices.get(key))
		{
			if !selection
				.iter()
				.any(|key| key.as_str() == self.targets[*index].key)
			{
				box_lines(
					&mut visuals,
					self.targets[*index].bounds,
					[0.7, 0.8, 1.0, 0.6],
					dpi as f32,
				);
			}
		}
		if selection.len() > 1024 {
			if let Some(bounds) = self.selected_bounds(workspace) {
				box_lines(
					&mut visuals,
					bounds,
					[1.0, 0.72, 0.22, 1.0],
					1.5 * dpi as f32,
				);
			}
		} else {
			for key in selection {
				if let Some(&index) = self.target_indices.get(key.as_str()) {
					box_lines(
						&mut visuals,
						self.targets[index].bounds,
						[1.0, 0.72, 0.22, 1.0],
						1.5 * dpi as f32,
					);
				}
			}
		}
		if let Some(gizmo) = self.gizmo(workspace, extent, dpi)? {
			for axis in gizmo.axes {
				let half = axis * gizmo.size * 0.045;
				visuals.lines.push(EditorLine {
					start: (gizmo.pivot - half).to_array(),
					end: (gizmo.pivot + half).to_array(),
					color: [1.0, 0.86, 0.4, 1.0],
					width: 3.0 * dpi as f32,
					depth_test: false,
				});
			}
			for segment in gizmo.segments(self.settings.tool) {
				let color = if self.hover_handle == Some(segment.handle) {
					[1.0, 0.86, 0.4, 1.0]
				} else {
					match segment.axis {
						0 => [0.94, 0.26, 0.24, 1.0],
						1 => [0.36, 0.85, 0.49, 1.0],
						2 => [0.3, 0.57, 1.0, 1.0],
						_ => [1.0, 0.8, 0.3, 1.0],
					}
				};
				visuals.lines.push(EditorLine {
					start: segment.a.to_array(),
					end: segment.b.to_array(),
					color,
					width: 2.2 * dpi as f32,
					depth_test: false,
				});
			}
		}
		if let Some(position) = self
			.placement
			.as_ref()
			.and_then(|placement| placement.position)
		{
			let bounds = Bounds3::new(
				(position - Vec3::new(0.5, 0.0, 0.5)).to_array(),
				(position + Vec3::new(0.5, 1.0, 0.5)).to_array(),
			)?;
			box_lines(&mut visuals, bounds, [0.4, 1.0, 0.7, 0.9], 2.0 * dpi as f32);
		}
		if let Some(click) = self.click.as_ref() {
			if (click.current[0] - click.start[0]).hypot(click.current[1] - click.start[1])
				> 4.0 * dpi
			{
				let matrix = self
					.camera
					.view_projection(extent, self.settings.projection)?
					.inverse();
				let points = [
					[click.start[0], click.start[1]],
					[click.current[0], click.start[1]],
					click.current,
					[click.start[0], click.current[1]],
				];
				let world = points.map(|point| {
					let p = matrix
						* Vec4::new(
							(point[0] / extent[0] as f64 * 2.0 - 1.0) as f32,
							(point[1] / extent[1] as f64 * 2.0 - 1.0) as f32,
							0.1,
							1.0,
						);
					p.truncate() / p.w
				});
				for i in 0..4 {
					visuals.lines.push(EditorLine {
						start: world[i].to_array(),
						end: world[(i + 1) % 4].to_array(),
						color: [0.5, 0.75, 1.0, 1.0],
						width: 1.5 * dpi as f32,
						depth_test: false,
					});
				}
			}
		}
		if self.settings.colliders {
			self.collider_visuals.append(&mut visuals, dpi);
		}
		Ok(visuals)
	}
}

fn box_lines(visuals: &mut EditorVisuals, bounds: Bounds3, color: [f32; 4], width: f32) {
	let corners = bounds_corners(bounds);
	for i in 0..8 {
		for axis in [1, 2, 4] {
			let other = i ^ axis;
			if other > i {
				visuals.lines.push(EditorLine {
					start: corners[i].to_array(),
					end: corners[other].to_array(),
					color,
					width,
					depth_test: false,
				});
			}
		}
	}
}
