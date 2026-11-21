use super::*;

type CursorGeometry = ([f64; 2], [u32; 2]);

impl GameApp {
	pub(crate) fn left_press(&mut self) -> Result<(), String> {
		if self.standalone_runtime || self.mouse_captured {
			return Ok(());
		}
		let Some((pixel, extent)) = self.cursor_geometry()? else {
			return Ok(());
		};
		let scale = self
			.window
			.as_ref()
			.map_or(1.0, |window| window.scale_factor());
		if self.tools.panel.contains(pixel, scale) {
			if let Some(action) = self.tools.panel.hit(pixel, scale) {
				self.panel_action(action)?;
			}
			return Ok(());
		}
		self.tools.panel.edit = None;
		if self.authored.is_some() {
			return Ok(());
		}
		if let Some(entity) = self.editor.selected_entity() {
			let pose = gizmo::GizmoPose::from_scene(&self.scene, entity)?;
			let mode = self.tools.panel.mode;
			if let Some(axis) = gizmo::hit_test_parented(&self.camera, extent, pose, mode, pixel) {
				self.tools.drag = Some(GizmoDrag::begin_parented(
					entity,
					pose,
					&self.camera,
					extent,
					pixel,
					mode,
					axis,
				)?);
				self.input.suppress_held();
				self.editor.end_gesture();
				return Ok(());
			}
		}
		self.pick_entity()
	}

	pub(crate) fn cursor_geometry(&mut self) -> Result<Option<CursorGeometry>, String> {
		let Some(pixel) = self.cursor_position else {
			return Ok(None);
		};
		let size = self
			.window
			.as_ref()
			.ok_or("Window is not initialized")?
			.inner_size();
		if size.width == 0 || size.height == 0 {
			return Ok(None);
		}
		let Some(extent) = self
			.renderer
			.as_mut()
			.ok_or("Renderer is not initialized")?
			.prepare()?
		else {
			return Ok(None);
		};
		let pixel = [
			pixel[0] * f64::from(extent[0]) / f64::from(size.width),
			pixel[1] * f64::from(extent[1]) / f64::from(size.height),
		];
		if self.standalone_runtime || self.editor.is_playing() {
			let rect = runtime::GameContentRect::fit(extent[0], extent[1])
				.ok_or("Game viewport extent is out of range")?;
			return Ok(rect
				.local_pointer(pixel)
				.map(|local| (local, rect.extent())));
		}
		Ok(Some((pixel, extent)))
	}

	pub(crate) fn preview_drag(&mut self) -> Result<(), String> {
		if self.tools.drag.is_none() {
			return Ok(());
		}
		let Some((pixel, _)) = self.cursor_geometry()? else {
			return Ok(());
		};
		let drag = self.tools.drag.as_ref().ok_or("Drag is missing")?;
		let after = drag.preview(pixel)?;
		*self
			.scene
			.get_mut(drag.entity)
			.ok_or("Dragged entity is missing")?
			.transform_mut() = after;
		Ok(())
	}

	pub(crate) fn finish_drag(&mut self) -> Result<(), String> {
		let Some(drag) = self.tools.drag.take() else {
			return Ok(());
		};
		let after = *self
			.scene
			.get(drag.entity)
			.ok_or("Dragged entity is missing")?
			.transform();
		self.editor
			.commit_transform(&mut self.scene, drag.entity, drag.before, after)?;
		self.tools.panel.status = "Transform applied. Ctrl+Z undoes the entire drag.".into();
		Ok(())
	}

	pub(crate) fn cancel_drag(&mut self) {
		if let Some(drag) = self.tools.drag.take()
			&& let Some(instance) = self.scene.get_mut(drag.entity)
		{
			*instance.transform_mut() = drag.before;
		}
	}
}
