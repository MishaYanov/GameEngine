use crate::{
	GameApp,
	editor_panel::{EditorPanel, PanelAction, Property},
	gizmo_overlay::GizmoOverlay,
};
use runtime::{
	SceneData, Transform,
	assets::catalog::{self, ModelAsset},
	gizmo::{self, GizmoDrag, GizmoMode},
	model_instance::ModelInstance,
	visibility::SceneRenderStats,
};
use std::collections::HashMap;
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

mod gestures;
pub(crate) use crate::frame_metrics::FrameMetrics;

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
