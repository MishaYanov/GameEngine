use crate::{
	Camera, EntitySelection, InputState, Key,
	commands::{CommandHistory, SceneEditEffect, SetTransform},
	model_instance::ModelInstance,
	picking::{self, CameraRay},
	scene::{EntityId, Scene},
};

#[derive(Default)]
pub struct SceneEditor {
	selection: EntitySelection,
	history: CommandHistory,
	movement_entity: Option<EntityId>,
}

impl SceneEditor {
	pub fn rename_selected(
		&mut self,
		scene: &mut Scene,
		name: impl Into<String>,
	) -> Result<(), String> {
		let entity = self.selected_entity().ok_or("Select an entity first")?;
		self.history.rename(scene, entity, name)?;
		self.end_gesture();
		Ok(())
	}

	pub fn set_selected_transform(
		&mut self,
		scene: &mut Scene,
		transform: crate::Transform,
	) -> Result<(), String> {
		let entity = self.selected_entity().ok_or("Select an entity first")?;
		self.history
			.execute(scene, SetTransform { entity, transform }, false)?;
		self.end_gesture();
		Ok(())
	}

	/// Commit a preview as one undo record; restore the initial value first.
	pub fn commit_transform(
		&mut self,
		scene: &mut Scene,
		entity: EntityId,
		before: crate::Transform,
		after: crate::Transform,
	) -> Result<(), String> {
		before.validate()?;
		after.validate()?;
		*scene
			.get_mut(entity)
			.ok_or("Dragged entity is missing")?
			.transform_mut() = before;
		self.history.execute(
			scene,
			SetTransform {
				entity,
				transform: after,
			},
			false,
		)?;
		self.end_gesture();
		Ok(())
	}
	pub fn from_scene(scene: &Scene) -> Self {
		let mut editor = Self::default();
		editor.selection.select_next(scene);
		editor
	}

	pub fn selected_entity(&self) -> Option<EntityId> {
		self.selection.selected()
	}

	pub fn select(&mut self, scene: &Scene, id: EntityId) -> Result<(), String> {
		self.selection.select(scene, id)?;
		self.end_gesture();
		Ok(())
	}

	pub fn end_gesture(&mut self) {
		self.movement_entity = None;
	}

	pub fn pick(
		&mut self,
		scene: &Scene,
		camera: &Camera,
		pixel: [f64; 2],
		extent: [u32; 2],
	) -> Result<Option<EntityId>, String> {
		let selected = picking::pick(scene, CameraRay::from_screen(camera, pixel, extent)?);
		match selected {
			Some(id) => self.select(scene, id)?,
			None => {
				self.selection.clear();
				self.end_gesture();
			}
		}
		Ok(selected)
	}

	pub fn update(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		delta_seconds: f32,
	) -> Result<(), String> {
		let previous_movement = self.movement_entity.take();
		self.selection.clear_if_missing(scene);
		if input.just_pressed(Key::Tab) {
			self.selection.select_next(scene);
		}
		let control = input.is_held(Key::ControlLeft) || input.is_held(Key::ControlRight);
		if control && (input.just_pressed(Key::Z) || input.just_pressed(Key::Y)) {
			let effect = if input.just_pressed(Key::Z) {
				self.history.undo_scene(scene)?
			} else {
				self.history.redo_scene(scene)?
			};
			if let Some(effect) = effect {
				self.select_effect(scene, effect)?;
			}
			return Ok(());
		}
		let Some(entity) = self.selection.selected() else {
			return Ok(());
		};
		let before = *scene
			.get(entity)
			.ok_or("Selected entity is missing")?
			.transform();
		let mut after = before;
		let direction = movement_direction(input);
		after.position[0] += direction[0] * delta_seconds;
		after.position[1] += direction[1] * delta_seconds;
		let rotation_step = 5.0_f32.to_radians();
		let mut rotation_delta = 0.0;
		if input.just_pressed(Key::Q) {
			rotation_delta -= rotation_step;
		}
		if input.just_pressed(Key::E) {
			rotation_delta += rotation_step;
		}
		after.rotation[2] += rotation_delta;
		let scale = match (input.just_pressed(Key::R), input.just_pressed(Key::F)) {
			(true, false) => 1.1,
			(false, true) => 1.0 / 1.1,
			_ => 1.0,
		};
		for component in &mut after.scale {
			*component *= scale;
		}
		if before == after {
			return Ok(());
		}
		let discrete = [Key::Q, Key::E, Key::R, Key::F]
			.into_iter()
			.any(|key| input.just_pressed(key));
		let fresh_movement = [Key::W, Key::A, Key::S, Key::D]
			.into_iter()
			.any(|key| input.just_pressed(key));
		let movement_only = before.position != after.position && !discrete;
		let merge = movement_only
			&& previous_movement == Some(entity)
			&& !fresh_movement
			&& !input.just_pressed(Key::Tab);
		self.history.execute(
			scene,
			SetTransform {
				entity,
				transform: after,
			},
			merge,
		)?;
		if movement_only {
			self.movement_entity = Some(entity);
		}
		Ok(())
	}

	pub fn spawn(
		&mut self,
		scene: &mut Scene,
		name: impl Into<String>,
		path: impl Into<String>,
		instance: ModelInstance,
	) -> Result<EntityId, String> {
		let name = name.into();
		let path = path.into();
		if name.trim().is_empty()
			|| path.trim().is_empty()
			|| path.starts_with('/')
			|| path.contains(['\\', ':'])
			|| path.split('/').any(|part| part == "..")
		{
			return Err("An entity needs a name and an asset-relative model path".to_string());
		}
		let id = self.history.spawn(scene, name, path, instance)?;
		self.select(scene, id)?;
		Ok(id)
	}

	pub fn duplicate_selected(&mut self, scene: &mut Scene) -> Result<Option<EntityId>, String> {
		self.selection.clear_if_missing(scene);
		let Some(source) = self.selection.selected() else {
			return Ok(None);
		};
		let id = self.history.duplicate(scene, source, [0.4, 0.0, 0.0])?;
		self.select(scene, id)?;
		Ok(Some(id))
	}

	pub fn delete_selected(&mut self, scene: &mut Scene) -> Result<(), String> {
		self.selection.clear_if_missing(scene);
		let Some(entity) = self.selection.selected() else {
			return Ok(());
		};
		self.history.delete(scene, entity)?;
		self.selection.clear();
		self.selection.select_next(scene);
		self.end_gesture();
		Ok(())
	}

	fn select_effect(&mut self, scene: &Scene, effect: SceneEditEffect) -> Result<(), String> {
		if effect.present {
			self.select(scene, effect.entity)?;
		} else {
			self.selection.clear_if_missing(scene);
			if self.selection.selected().is_none() {
				self.selection.select_next(scene);
			}
		}
		Ok(())
	}
}

pub fn movement_direction(input: &InputState) -> [f32; 2] {
	let mut direction = [0.0_f32; 2];
	if input.is_held(Key::W) {
		direction[1] += 1.0;
	}
	if input.is_held(Key::S) {
		direction[1] -= 1.0;
	}
	if input.is_held(Key::A) {
		direction[0] -= 1.0;
	}
	if input.is_held(Key::D) {
		direction[0] += 1.0;
	}
	let length = direction[0].hypot(direction[1]);
	if length > 0.0 {
		direction[0] /= length;
		direction[1] /= length;
	}
	direction
}
