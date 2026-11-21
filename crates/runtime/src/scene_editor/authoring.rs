//! Native selection, authoring gestures, and structural commands.

use super::SceneEditor;
use crate::{
	Camera, InputState, Key, Transform,
	editor::command::{
		EditorCommandDto as Command, EntitySpecDto, TransformDto, TransformEditDto, TransformSpace,
	},
	input::MovementBindings,
	model_instance::ModelInstance,
	picking::{self, CameraRay},
	scene::{EntityId, MAX_SCENE_RENDER_OBJECTS, Scene},
};

impl SceneEditor {
	pub(super) fn refresh_selection(&mut self, scene: &Scene) {
		self.selected = self
			.session
			.as_ref()
			.and_then(|session| session.selection().first())
			.and_then(|key| scene.entity_by_key(key));
	}

	pub(super) fn finish_gesture(&mut self) -> Result<(), String> {
		if self.movement_entity.is_some() {
			self.execute_core(Command::CommitTransaction)?;
			self.movement_entity = None;
		}
		Ok(())
	}

	pub fn end_gesture(&mut self) {
		if let Err(error) = self.finish_gesture() {
			engine_logger::error!("Failed to finish native movement command: {error}");
		}
	}

	pub fn selected_entity(&self) -> Option<EntityId> {
		self.selected
	}

	pub fn select(&mut self, scene: &Scene, id: EntityId) -> Result<(), String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		let key = scene
			.key(id)
			.ok_or("Selected entity is missing or has no persistent key")?;
		self.execute_core(Command::Select {
			keys: vec![key.as_str().to_owned()],
		})?;
		self.refresh_selection(scene);
		Ok(())
	}

	pub fn pick(
		&mut self,
		scene: &Scene,
		camera: &Camera,
		pixel: [f64; 2],
		extent: [u32; 2],
	) -> Result<Option<EntityId>, String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		let selected = picking::pick(scene, CameraRay::from_screen(camera, pixel, extent)?);
		let keys = selected
			.and_then(|id| scene.key(id))
			.map(|key| vec![key.as_str().to_owned()])
			.unwrap_or_default();
		self.execute_core(Command::Select { keys })?;
		self.refresh_selection(scene);
		Ok(selected)
	}

	pub fn rename_selected(
		&mut self,
		scene: &mut Scene,
		name: impl Into<String>,
	) -> Result<(), String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		let key = self.selected_key(scene)?;
		self.apply(
			scene,
			Command::Rename {
				key,
				name: name.into(),
			},
			None,
		)
	}

	fn selected_key(&self, scene: &Scene) -> Result<String, String> {
		scene
			.key(self.selected.ok_or("Select an entity first")?)
			.map(|key| key.as_str().to_owned())
			.ok_or_else(|| "Selected entity is missing".into())
	}

	fn transform_command(key: String, transform: Transform) -> Command {
		Command::SetTransforms {
			edits: vec![TransformEditDto {
				key,
				transform: TransformDto {
					position: transform.position,
					rotation: transform.rotation,
					scale: transform.scale,
				},
			}],
			space: TransformSpace::Local,
			snap: false,
		}
	}

	pub fn set_selected_transform(
		&mut self,
		scene: &mut Scene,
		transform: Transform,
	) -> Result<(), String> {
		transform.validate()?;
		self.initialize(scene)?;
		self.finish_gesture()?;
		let key = self.selected_key(scene)?;
		self.apply(scene, Self::transform_command(key, transform), None)
	}

	pub fn commit_transform(
		&mut self,
		scene: &mut Scene,
		entity: EntityId,
		before: Transform,
		after: Transform,
	) -> Result<(), String> {
		before.validate()?;
		after.validate()?;
		self.initialize(scene)?;
		self.finish_gesture()?;
		let key = scene
			.key(entity)
			.ok_or("Dragged entity is missing")?
			.as_str()
			.to_owned();
		*scene
			.get_mut(entity)
			.ok_or("Dragged entity is missing")?
			.transform_mut() = before;
		self.apply(scene, Self::transform_command(key, after), None)
	}

	pub fn update(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		delta_seconds: f32,
	) -> Result<(), String> {
		if !delta_seconds.is_finite() || delta_seconds < 0.0 {
			return Err("Editor frame time must be finite and nonnegative".into());
		}
		self.initialize(scene)?;
		if self.selected.is_some_and(|id| !scene.contains(id)) {
			self.refresh_selection(scene);
		}
		if input.just_pressed(Key::Tab) {
			let ids: Vec<_> = scene.entity_ids().collect();
			if !ids.is_empty() {
				let index = self
					.selected
					.and_then(|selected| ids.iter().position(|id| *id == selected))
					.map_or(0, |index| (index + 1) % ids.len());
				self.select(scene, ids[index])?;
			}
		}
		let control = input.is_held(Key::ControlLeft) || input.is_held(Key::ControlRight);
		if control && (input.just_pressed(Key::Z) || input.just_pressed(Key::Y)) {
			self.finish_gesture()?;
			return self.apply(
				scene,
				if input.just_pressed(Key::Z) {
					Command::Undo
				} else {
					Command::Redo
				},
				None,
			);
		}
		let Some(entity) = self.selected else {
			self.finish_gesture()?;
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
		let rotation = 5.0_f32.to_radians();
		after.rotation[2] += if input.just_pressed(Key::E) {
			rotation
		} else {
			0.0
		} - if input.just_pressed(Key::Q) {
			rotation
		} else {
			0.0
		};
		let scale = match (input.just_pressed(Key::R), input.just_pressed(Key::F)) {
			(true, false) => 1.1,
			(false, true) => 1.0 / 1.1,
			_ => 1.0,
		};
		for component in &mut after.scale {
			*component *= scale;
		}
		if before == after {
			self.finish_gesture()?;
			return Ok(());
		}
		after.validate()?;
		let discrete = [Key::Q, Key::E, Key::R, Key::F]
			.into_iter()
			.any(|key| input.just_pressed(key));
		let fresh = [Key::W, Key::A, Key::S, Key::D]
			.into_iter()
			.any(|key| input.just_pressed(key));
		let movement_only = before.position != after.position && !discrete;
		if !movement_only
			|| self.movement_entity != Some(entity)
			|| fresh || input.just_pressed(Key::Tab)
		{
			self.finish_gesture()?;
		}
		if movement_only && self.movement_entity.is_none() {
			self.execute_core(Command::BeginTransaction {
				label: "Move entity".into(),
			})?;
			self.movement_entity = Some(entity);
		}
		let key = self.selected_key(scene)?;
		self.apply(scene, Self::transform_command(key, after), None)
	}

	fn check_additional_objects(scene: &Scene, additional: usize) -> Result<(), String> {
		let current = scene
			.instances()
			.try_fold(0usize, |count, instance| {
				count.checked_add(instance.model().parts().len())
			})
			.ok_or("Render-object count overflow")?;
		if current
			.checked_add(additional)
			.is_none_or(|count| count > MAX_SCENE_RENDER_OBJECTS)
		{
			return Err(format!(
				"Scene exceeds {MAX_SCENE_RENDER_OBJECTS} render objects"
			));
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
		self.initialize(scene)?;
		self.finish_gesture()?;
		Self::check_additional_objects(scene, instance.model().parts().len())?;
		let transform = *instance.transform();
		self.apply(
			scene,
			Command::Spawn {
				entity: EntitySpecDto {
					asset_id: None,
					name: name.into(),
					model_path: path.into(),
					transform: TransformDto {
						position: transform.position,
						rotation: transform.rotation,
						scale: transform.scale,
					},
					parent: None,
					mesh_index: None,
				},
			},
			Some(&instance),
		)?;
		self.selected
			.ok_or_else(|| "Spawned entity selection is missing".into())
	}

	pub fn duplicate_selected(&mut self, scene: &mut Scene) -> Result<Option<EntityId>, String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		if self.selected.is_none() {
			return Ok(None);
		}
		let additional =
			scene.subtree_render_object_count(self.selected.expect("selection checked"))?;
		Self::check_additional_objects(scene, additional)?;
		self.apply(
			scene,
			Command::DuplicateSelection {
				offset: [0.4, 0.0, 0.0],
			},
			None,
		)?;
		Ok(self.selected)
	}

	pub fn delete_selected(&mut self, scene: &mut Scene) -> Result<(), String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		if self.selected.is_none() {
			return Ok(());
		}
		self.apply(scene, Command::DeleteSelection, None)?;
		if self.selected.is_none()
			&& let Some(id) = scene.entity_ids().next()
		{
			self.select(scene, id)?;
		}
		Ok(())
	}
}

pub fn movement_direction(input: &InputState) -> [f32; 2] {
	MovementBindings::WASD.direction(input)
}
