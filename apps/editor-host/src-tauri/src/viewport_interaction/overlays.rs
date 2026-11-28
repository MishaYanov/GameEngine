use super::*;

const SELECTED_BOUNDS_COLOR: [f32; 4] = [1.0, 0.72, 0.22, 0.85];
// At most 24 lines per selection. Leave room for colliders and the active gizmo.
const MAX_INDIVIDUAL_SELECTION_BOUNDS: usize = renderer::MAX_EDITOR_LINES / 32;

const GIZMO_AXIS_COLORS: [[f32; 4]; 3] = [
	[1.0, 0.33, 0.29, 1.0],
	[0.40, 0.91, 0.51, 1.0],
	[0.38, 0.67, 1.0, 1.0],
];

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
		if selection.len() == 1 {
			let cache = self.transform_targets(workspace)?;
			let segments = runtime::GameContentRect::fit(extent[0], extent[1]).and_then(|rect| {
				tools::camera_frustum(cache.active_camera.as_ref()?, rect.aspect())
			});
			if let Some(segments) = segments {
				visuals
					.lines
					.extend(segments.map(|(start, end)| EditorLine {
						start: start.to_array(),
						end: end.to_array(),
						color: [0.44, 0.89, 1.0, 0.95],
						width: 1.45 * dpi as f32,
						depth_test: false,
					}));
			}
		}
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
		if !selection.is_empty() {
			let matrix = self
				.camera
				.view_projection(extent, self.settings.projection)?;
			if selection.len() > MAX_INDIVIDUAL_SELECTION_BOUNDS {
				if let Some(bounds) = self.selected_bounds(workspace) {
					selection_brackets(&mut visuals, bounds, matrix, extent, dpi as f32);
				}
			} else {
				for key in selection {
					if let Some(&index) = self.target_indices.get(key.as_str()) {
						selection_brackets(
							&mut visuals,
							self.targets[index].bounds,
							matrix,
							extent,
							dpi as f32,
						);
					}
				}
			}
		}
		if let Some(gizmo) = self.gizmo(workspace, extent, dpi)? {
			let highlighted = self
				.drag
				.as_ref()
				.map(GizmoDrag::handle)
				.or(self.hover_handle);
			let mut strokes = Vec::new();
			for segment in gizmo.visible_segments(
				&self.camera,
				extent,
				self.settings.projection,
				self.settings.tool,
			) {
				let active = highlighted == Some(segment.handle);
				let rear_ring = matches!(segment.handle, tools::Handle::Rotate(_))
					&& ((segment.a + segment.b) * 0.5 - gizmo.pivot).dot(self.camera.forward())
					> gizmo.size * 0.04;
				let (mut color, width) = match segment.handle {
					tools::Handle::Screen | tools::Handle::UniformScale => {
						([0.86, 0.91, 0.98, 1.0], 2.1)
					}
					tools::Handle::Plane(_, _) => (GIZMO_AXIS_COLORS[segment.axis], 1.8),
					_ => (GIZMO_AXIS_COLORS[segment.axis], 2.6),
				};
				if active {
					color = [1.0, 0.86, 0.4, 1.0];
				} else if rear_ring {
					color[3] = 0.46;
				}
				let width = if active { width + 0.8 } else { width };
				let line = EditorLine {
					start: segment.a.to_array(),
					end: segment.b.to_array(),
					color,
					width: (width * dpi as f32).min(32.0),
					depth_test: false,
				};
				visuals.lines.push(EditorLine {
					color: [
						0.025,
						0.032,
						0.05,
						if rear_ring && !active { 0.65 } else { 0.94 },
					],
					width: ((width + 2.0) * dpi as f32).min(32.0),
					..line
				});
				strokes.push((active, line));
			}
			// Keep the selected handle readable where gizmo shapes cross.
			visuals.lines.extend(
				strokes
					.iter()
					.filter_map(|(active, line)| (!active).then_some(*line)),
			);
			visuals.lines.extend(
				strokes
					.into_iter()
					.filter_map(|(active, line)| active.then_some(line)),
			);
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

fn selection_brackets(
	visuals: &mut EditorVisuals,
	bounds: Bounds3,
	matrix: Mat4,
	extent: [u32; 2],
	dpi: f32,
) {
	let corners = bounds_corners(bounds);
	let projected = corners.map(|corner| {
		tools::project(matrix, extent, corner).map(|point| (point, (matrix * corner.extend(1.0)).w))
	});

	for (index, corner) in corners.into_iter().enumerate() {
		let Some((origin, start_w)) = projected[index] else {
			continue;
		};
		for axis in [1, 2, 4] {
			let other_index = index ^ axis;
			let Some((other, end_w)) = projected[other_index] else {
				continue;
			};
			let length = origin.distance(other);
			if length < 1.0 {
				continue;
			}
			let screen_fraction = (14.0 * dpi / length).min(0.22);
			// Convert the desired projected fraction to a world-edge fraction.
			let world_fraction = screen_fraction * start_w
				/ ((1.0 - screen_fraction) * end_w + screen_fraction * start_w);
			visuals.lines.push(EditorLine {
				start: corner.to_array(),
				end: corner.lerp(corners[other_index], world_fraction).to_array(),
				color: SELECTED_BOUNDS_COLOR,
				width: (1.35 * dpi).min(32.0),
				depth_test: false,
			});
		}
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
