use super::*;

impl GameApp {
	pub(super) fn render(&mut self) -> Result<renderer::DrawStatus, String> {
		let hud_rows = if self.standalone_runtime {
			gameplay_hud_rows(self.editor.play_status().as_ref())
		} else {
			Vec::new()
		};
		if let Some(camera) = self.editor.play_rig_camera() {
			self.camera = camera;
		}
		let dirty = !self.standalone_runtime && self.is_dirty();
		let Some(renderer) = self.renderer.as_mut() else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let Some([width, height]) = renderer.prepare()? else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let game_rect = if self.standalone_runtime || self.editor.is_playing() {
			Some(
				GameContentRect::fit(width, height)
					.ok_or("Game viewport extent is out of range")?,
			)
		} else {
			None
		};
		let content_extent = game_rect.map_or([width, height], GameContentRect::extent);
		let camera_uniform = CameraUniform::perspective(
			self.camera.position,
			self.camera.target(),
			[0.0, 1.0, 0.0],
			self.camera.vertical_fov,
			game_rect.map_or(width as f32 / height as f32, GameContentRect::aspect),
			self.camera.near,
			self.camera.far,
		);

		let frustum = if self.tools.culling {
			Some(runtime::visibility::Frustum::from_view_projection(
				glam::Mat4::from_cols_array(&camera_uniform.view_projection),
			)?)
		} else {
			None
		};
		let (mut objects, mut stats) = self.scene.try_render_visible(frustum.as_ref())?;
		if !self.standalone_runtime
			&& self.authored.is_none()
			&& let Some(object) = self
				.selection_overlay
				.as_ref()
				.and_then(|overlay| overlay.object(&self.scene, &self.editor))
		{
			stats.draw_calls += 1;
			stats.triangles += object.mesh().index_count() as usize / 3;
			objects.push(object);
		}

		if !self.standalone_runtime {
			if self.authored.is_none()
				&& let Some(transform) = self
					.editor
					.selected_entity()
					.map(|id| runtime::gizmo::GizmoPose::from_scene(&self.scene, id))
					.transpose()?
				&& let Some(gizmo) = self.tools.gizmo.as_mut()
			{
				gizmo.prepare(
					renderer,
					&self.camera,
					content_extent,
					transform,
					self.tools.panel.mode,
					self.tools.drag.as_ref().map(|drag| drag.axis),
				)?;
				let object = gizmo.object();
				stats.draw_calls += 1;
				stats.triangles += object.mesh().index_count() as usize / 3;
				objects.push(object);
			}
			let scale = self
				.window
				.as_ref()
				.map_or(1.0, |window| window.scale_factor());
			// The panel is one batched draw. Include its previous geometry in
			// displayed totals; the current text has the same quad count.
			if let Some(panel) = self.tools.panel.object() {
				stats.draw_calls += 1;
				stats.triangles += panel.mesh().index_count() as usize / 3;
			}
			self.tools.panel.prepare(
				renderer,
				content_extent,
				scale,
				PanelView {
					scene: &self.scene,
					editor: &self.editor,
					models: &self.tools.models,
					playing: self.authored.is_some(),
					dirty,
					stats,
					frame_ms: self.tools.metrics.frame_ms,
					cpu_ms: self.tools.metrics.cpu_ms,
					culling: self.tools.culling,
				},
			)?;
			if let Some(panel) = self.tools.panel.object() {
				objects.push(panel);
			}
		}
		if self.standalone_runtime && !hud_rows.is_empty() {
			if self.game_hud.is_none() {
				self.game_hud = Some(renderer::ui::ScreenOverlay::new(renderer)?);
			}
			let hud = self.game_hud.as_mut().expect("initialized HUD");
			let scale = self
				.window
				.as_ref()
				.map_or(1.0, |window| window.scale_factor());
			hud.prepare(renderer, content_extent, scale, &hud_rows)?;
			if let Some(object) = hud.object() {
				objects.push(object);
			}
		}
		self.tools.stats = stats;

		renderer.set_lights(&self.scene.renderer_lights())?;
		match game_rect {
			Some(rect) => renderer.draw_in_rect(&camera_uniform, &objects, rect.pixels()),
			None => renderer.draw(&camera_uniform, &objects),
		}
	}
}

pub(super) fn gameplay_hud_rows(
	status: Option<&runtime::editor::command::PlayStatusDto>,
) -> Vec<String> {
	let Some(status) = status else {
		return Vec::new();
	};
	let mut rows = Vec::new();
	if let Some(gameplay) = &status.gameplay {
		rows.push(gameplay.objective.clone());
		if let Some(detail) = &gameplay.detail {
			rows.push(detail.clone());
		}
		if let Some(score) = gameplay.score {
			rows.push(format!("Score {score}"));
		}
		if gameplay.completed {
			rows.push("F10 to restart".into());
		}
	}
	if status.paused {
		rows.push("Paused".into());
	}
	if let Some(scene) = &status.pending_scene_id {
		rows.push(format!("Scene {} pending. F8 to retry", scene.as_str()));
	}
	rows
}
