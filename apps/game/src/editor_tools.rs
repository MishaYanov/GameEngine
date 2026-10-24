use crate::{
	GameApp,
	editor_panel::{EditorPanel, PanelAction, Property},
	gizmo_overlay::GizmoOverlay,
};
type CursorGeometry = ([f64; 2], [u32; 2]);
use runtime::{
	SceneData, Transform,
	assets::catalog::{self, ModelAsset},
	gizmo::{self, GizmoDrag, GizmoMode},
	model_instance::ModelInstance,
	visibility::SceneRenderStats,
};
use std::{collections::HashMap, time::Instant};
use winit::{event::KeyEvent, keyboard::KeyCode};

pub(crate) struct EditorTools {
	pub panel: EditorPanel,
	pub gizmo: Option<GizmoOverlay>,
	pub models: Vec<ModelAsset>,
	pub templates: HashMap<String, ModelInstance>,
	pub saved: Option<SceneData>,
	pub drag: Option<GizmoDrag>,
	pub culling: bool,
	pub stats: SceneRenderStats,
	pub metrics: FrameMetrics,
}

impl Default for EditorTools {
	fn default() -> Self {
		Self {
			panel: EditorPanel::default(),
			gizmo: None,
			models: Vec::new(),
			templates: HashMap::new(),
			saved: None,
			drag: None,
			culling: true,
			stats: SceneRenderStats::default(),
			metrics: FrameMetrics::default(),
		}
	}
}

#[derive(Default)]
pub(crate) struct FrameMetrics {
	last: Option<Instant>,
	samples: u32,
	elapsed: f32,
	update: f32,
	pub frame_ms: f32,
	pub cpu_ms: f32,
}

impl FrameMetrics {
	pub fn record(&mut self, now: Instant, update_seconds: f32) {
		if let Some(last) = self.last.replace(now) {
			let delta = now.duration_since(last).as_secs_f32();
			// Focus/minimization stalls are not normal frame samples.
			if delta > 0.25 {
				self.samples = 0;
				self.elapsed = 0.0;
				self.update = 0.0;
				return;
			}
			self.samples += 1;
			self.elapsed += delta;
			self.update += update_seconds;
			if self.elapsed >= 0.5 {
				self.frame_ms = self.elapsed * 1000.0 / self.samples as f32;
				self.cpu_ms = self.update * 1000.0 / self.samples as f32;
				self.samples = 0;
				self.elapsed = 0.0;
				self.update = 0.0;
			}
		}
	}
}

impl GameApp {
	pub(crate) fn is_dirty(&self) -> bool {
		let (scene, camera) = self
			.authored
			.as_ref()
			.map_or((&self.scene, &self.camera), |authored| {
				(&authored.scene, &authored.camera)
			});
		self.editor.project() != self.project.as_ref()
			|| self
				.tools
				.saved
				.as_ref()
				.is_some_and(|saved| !scene.matches_data(saved, camera))
	}

	pub(crate) fn refresh_assets(&mut self) -> Result<(), String> {
		let root = self
			.asset_root
			.as_ref()
			.ok_or("Assets are not initialized")?;
		let models = catalog::scan_models(root)?;
		self.tools.panel.status = format!(
			"Found {} model assets. Place adds one undoable entity.",
			models.len()
		);
		self.tools.models = models;
		Ok(())
	}

	pub(crate) fn place_model(&mut self, path: &str) -> Result<(), String> {
		if self.authored.is_some() {
			return Err("Stop Play before placing a model".into());
		}
		let asset = self
			.tools
			.models
			.iter()
			.find(|asset| asset.path == path)
			.ok_or("Choose a model from the asset catalog")?
			.clone();
		self.wait_for_gpu()?;
		if !self.tools.templates.contains_key(path) {
			let renderer = self
				.renderer
				.as_ref()
				.ok_or("Renderer is not initialized")?;
			let assets = self
				.asset_manager
				.as_mut()
				.ok_or("Assets are not initialized")?;
			let model = assets
				.load_model(renderer, path)
				.map_err(|e| e.to_string())?;
			let instance = ModelInstance::new(std::sync::Arc::new(model), Transform::default());
			self.tools.templates.insert(path.into(), instance);
		}
		let mut instance = self
			.tools
			.templates
			.get(path)
			.ok_or("Model template is missing")?
			.clone();
		let forward = glam::Vec3::from_array(self.camera.forward());
		instance.transform_mut().position =
			(glam::Vec3::from_array(self.camera.position) + forward * 3.0).to_array();
		self.editor
			.spawn(&mut self.scene, asset.name, path, instance)?;
		self.tools.panel.status =
			format!("Placed {path} three units in front of the camera. Ctrl+Z undoes.");
		self.update_selection_title();
		Ok(())
	}

	pub(crate) fn panel_action(&mut self, action: PanelAction) -> Result<(), String> {
		if self.standalone_runtime {
			return Err("Editor panels are unavailable in Player mode".into());
		}
		if self.authored.is_some()
			&& !matches!(
				action,
				PanelAction::TogglePlay | PanelAction::Save | PanelAction::Reload
			) {
			return Err("Stop Play before editing the scene".into());
		}
		// Mouse commands have the same timing boundary as keyboard commands:
		// model loading or file I/O must not become movement on the next frame.
		self.clock.reset();
		self.input.suppress_held();
		self.editor.end_gesture();
		self.tools.panel.edit = None;
		match action {
			PanelAction::Select(id) => self.editor.select(&self.scene, id)?,
			PanelAction::Edit(property) => {
				if self.authored.is_none() {
					self.input.suppress_held();
					self.editor.end_gesture();
					self.tools
						.panel
						.begin_edit(&self.scene, &self.editor, property);
				}
			}
			PanelAction::Mode(mode) => {
				self.tools.panel.mode = mode;
			}
			PanelAction::Place(path) => self.place_model(&path)?,
			PanelAction::RefreshAssets => self.refresh_assets()?,
			PanelAction::TogglePlay => self.toggle_play()?,
			PanelAction::Save => self.save_scene()?,
			PanelAction::Reload => self.reload_scene()?,
		}
		self.update_selection_title();
		Ok(())
	}

	pub(crate) fn left_press(&mut self) -> Result<(), String> {
		if self.standalone_runtime || self.mouse_captured {
			return Ok(());
		}
		let Some(pixel) = self.cursor_position else {
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
		let Some((pixel, extent)) = self.cursor_geometry()? else {
			return Ok(());
		};
		if let Some(entity) = self.editor.selected_entity()
			&& let Some(instance) = self.scene.get(entity)
		{
			let before = *instance.transform();
			let mode = self.tools.panel.mode;
			if let Some(axis) = gizmo::hit_test(&self.camera, extent, before, mode, pixel) {
				self.tools.drag = Some(GizmoDrag::begin(
					entity,
					before,
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
		Ok(Some((
			[
				pixel[0] * extent[0] as f64 / size.width as f64,
				pixel[1] * extent[1] as f64 / size.height as f64,
			],
			extent,
		)))
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

	pub(crate) fn commit_property(&mut self) -> Result<(), String> {
		let Some(edit) = self.tools.panel.edit.take() else {
			return Ok(());
		};
		if self.authored.is_some() {
			return Err("Stop Play before editing properties".into());
		}
		self.editor.select(&self.scene, edit.entity)?;
		if matches!(edit.property, Property::Name) {
			self.editor.rename_selected(&mut self.scene, edit.text)?;
		} else {
			let value = edit
				.text
				.parse::<f32>()
				.map_err(|_| "Enter a finite number, for example 0.5")?;
			if !value.is_finite() {
				return Err("Property value must be finite".into());
			}
			let mut transform = *self
				.scene
				.get(edit.entity)
				.ok_or("Edited entity is missing")?
				.transform();
			match edit.property {
				Property::Position(axis) => transform.position[axis] = value,
				Property::Rotation(axis) => transform.rotation[axis] = value.to_radians(),
				Property::Scale(axis) => transform.scale[axis] = value,
				Property::Name => unreachable!(),
			}
			self.editor
				.set_selected_transform(&mut self.scene, transform)?;
		}
		self.tools.panel.status = "Property applied. Ctrl+Z undoes.".into();
		self.update_selection_title();
		Ok(())
	}

	pub(crate) fn editor_key(&mut self, code: KeyCode, event: &KeyEvent) -> bool {
		if self.standalone_runtime {
			return false;
		}
		if self.tools.panel.edit.is_some() {
			match code {
				KeyCode::Escape => {
					self.tools.panel.edit = None;
				}
				KeyCode::Enter | KeyCode::NumpadEnter => {
					if let Err(error) = self.commit_property() {
						self.report_editor_error(error);
					}
				}
				KeyCode::Backspace => {
					if let Some(edit) = self.tools.panel.edit.as_mut() {
						if edit.replace {
							edit.text.clear();
							edit.replace = false;
						} else {
							edit.text.pop();
						}
					}
				}
				KeyCode::KeyA
					if self.input.is_held(runtime::Key::ControlLeft)
						|| self.input.is_held(runtime::Key::ControlRight) =>
				{
					if let Some(edit) = self.tools.panel.edit.as_mut() {
						edit.replace = true;
					}
				}
				_ => {
					if let Some(text) = &event.text
						&& let Some(edit) = self.tools.panel.edit.as_mut()
					{
						let text: String = text.chars().filter(|c| !c.is_control()).collect();
						if !text.is_empty() {
							if edit.replace {
								edit.text.clear();
								edit.replace = false;
							}
							if edit.text.chars().count() + text.chars().count() <= 128 {
								edit.text.push_str(&text);
							}
						}
					}
					// Modifiers still reach InputState so Ctrl+A and release work.
					return !matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight);
				}
			}
			return true;
		}
		if self.tools.drag.is_some() {
			if code == KeyCode::Escape {
				self.cancel_drag();
				return true;
			}
			// Finish an active preview before mode/save/history commands.
			if matches!(
				code,
				KeyCode::F5 | KeyCode::F6 | KeyCode::F9 | KeyCode::KeyZ | KeyCode::KeyY
			) {
				self.cancel_drag();
			} else {
				return true;
			}
		}
		if !event.repeat {
			match code {
				KeyCode::F1 => {
					self.tools.panel.visible = !self.tools.panel.visible;
					return true;
				}
				KeyCode::F7 => {
					self.tools.culling = !self.tools.culling;
					return true;
				}
				KeyCode::Digit1 if self.authored.is_none() => {
					self.tools.panel.mode = GizmoMode::Translate;
					return true;
				}
				KeyCode::Digit2 if self.authored.is_none() => {
					self.tools.panel.mode = GizmoMode::Rotate;
					return true;
				}
				KeyCode::Digit3 if self.authored.is_none() => {
					self.tools.panel.mode = GizmoMode::Scale;
					return true;
				}
				_ => {}
			}
		}
		false
	}

	pub(crate) fn report_editor_error(&mut self, error: String) {
		engine_logger::error!("Editor operation failed: {error}");
		self.tools.panel.status = error;
	}
}
