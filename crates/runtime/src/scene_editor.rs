//! Compatibility adapter for the native development harness. Authoring and
//! undo/redo are delegated to the same CPU EditorSession used by Tauri.
use crate::input::MovementBindings;
use crate::{
	Camera, EntityKey, InputState, Key, Transform,
	editor::{
		EditorSession,
		command::{
			EditorCommandDto as Command, EditorCommandRequest, EntitySpecDto, TransformDto,
			TransformEditDto, TransformSpace,
		},
	},
	model_instance::ModelInstance,
	picking::{self, CameraRay},
	scene::{EntityId, EntitySnapshot, MAX_SCENE_RENDER_OBJECTS, Scene},
};
pub use engine_core::project::{BehaviorSpace, BehaviorSpec, ProjectData};
pub use engine_core::simulation_session::{MovementTarget, SimulationSession};
use std::collections::HashMap;

#[derive(Default)]
pub struct SceneEditor {
	session: Option<EditorSession>,
	resources: HashMap<EntityKey, EntitySnapshot>,
	selected: Option<EntityId>,
	movement_entity: Option<EntityId>,
	play_input_target: MovementTarget,
	play_movement_bindings: MovementBindings,
}

impl SceneEditor {
	pub fn for_project(
		scene: &Scene,
		camera: &Camera,
		project: ProjectData,
	) -> Result<Self, String> {
		let mut editor = Self {
			session: Some(EditorSession::for_project(scene.to_data(camera)?, project)?),
			..Self::default()
		};
		editor.capture_resources(scene)?;
		if let Some(id) = scene.entity_ids().next() {
			editor.select(scene, id)?;
		}
		Ok(editor)
	}

	pub fn project(&self) -> Option<&ProjectData> {
		self.session.as_ref().map(EditorSession::project)
	}

	pub fn mark_saved(&mut self) -> Result<(), String> {
		self.finish_gesture()?;
		self.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?
			.mark_saved()
	}

	/// Initialization/reload boundary; configuration is authored by the project
	/// document rather than inferred from robot names or keyboard bindings.
	pub fn set_initial_project(&mut self, project: ProjectData) -> Result<(), String> {
		self.finish_gesture()?;
		let old = self
			.session
			.as_ref()
			.ok_or("Native authoring session is not initialized")?;
		let mut next = EditorSession::for_project((*old.authored_document()).clone(), project)?;
		next.execute_command(EditorCommandRequest {
			expected_revision: next.revision(),
			command: Command::Select {
				keys: old
					.selection()
					.iter()
					.map(|key| key.as_str().to_owned())
					.collect(),
			},
		})?;
		self.session = Some(next);
		self.play_input_target = MovementTarget::default();
		Ok(())
	}

	pub fn is_playing(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::is_playing)
	}

	pub fn play_controlled_targets(&self) -> Option<&[EntityKey]> {
		self.session
			.as_ref()
			.and_then(EditorSession::play_controlled_targets)
	}

	pub fn simulation_paused(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::is_paused)
	}

	pub fn needs_tick(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::needs_tick)
	}

	pub fn start_play(&mut self, scene: &mut Scene) -> Result<(), String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		self.execute_core(Command::StartPlay)?;
		self.play_input_target = MovementTarget::default();
		let session = self.session.as_ref().expect("initialized session");
		scene.synchronize(&session.render_document())?;
		Ok(())
	}

	pub fn stop_play(&mut self, scene: &mut Scene) -> Result<(), String> {
		self.execute_core(Command::StopPlay)?;
		self.play_input_target = MovementTarget::default();
		let document = self
			.session
			.as_ref()
			.expect("initialized session")
			.authored_document();
		scene.synchronize_retained(&document, &self.resources, None)?;
		self.refresh_selection(scene);
		Ok(())
	}

	pub fn update_play(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		elapsed: f32,
	) -> Result<(), String> {
		self.initialize(scene)?;
		if input.just_pressed(Key::Space) {
			self.execute_core(Command::SetPaused {
				paused: !self.simulation_paused(),
			})?;
		}
		if self
			.play_controlled_targets()
			.is_some_and(|targets| !targets.is_empty())
		{
			let direction = self.play_movement_bindings.direction(input);
			self.route_play_input([direction[0], direction[1], 0.0])?;
		}

		let session = self.session.as_mut().expect("initialized session");
		let outcome = session.tick(elapsed as f64)?;
		if !outcome.changes.is_empty()
			&& let Some(world) = session.render_world()
			&& !scene.synchronize_world(world, &outcome.changes)?
		{
			return Err(
				"Configured simulation changed GPU topology; reload through the asset factory"
					.into(),
			);
		}
		Ok(())
	}

	pub fn from_scene(scene: &Scene) -> Self {
		let mut editor = Self::default();
		// The legacy constructor is infallible. Invalid transient content is
		// reported by the first command instead of installing a second history.
		if editor.initialize(scene).is_ok() {
			editor.refresh_selection(scene);
		}
		editor
	}

	pub fn set_play_input(&mut self, movement: [f32; 3]) -> Result<(), String> {
		self.execute_core(Command::SetPlayInput { movement })?;
		Ok(())
	}

	pub fn set_play_movement_bindings(
		&mut self,
		bindings: MovementBindings,
		input: &mut InputState,
	) -> bool {
		if self.play_movement_bindings == bindings {
			return false;
		}

		if let Some(session) = self.session.as_mut() {
			session.clear_play_input();
		}

		input.suppress_held();
		self.play_movement_bindings = bindings;

		true
	}

	pub fn set_entity_play_input(
		&mut self,
		entity: &EntityKey,
		movement: [f32; 3],
	) -> Result<(), String> {
		self.execute_core(Command::SetEntityInput {
			entity: entity.as_str().to_owned(),
			movement,
		})?;
		Ok(())
	}

	pub fn play_movement_bindings(&self) -> &MovementBindings {
		&self.play_movement_bindings
	}

	fn initialize(&mut self, scene: &Scene) -> Result<(), String> {
		if self.session.is_some() {
			return Ok(());
		}
		let data = scene.to_data(&Camera::default()).map_err(|error| {
			format!("Native editor requires persistent named scene data: {error}")
		})?;
		let mut session = EditorSession::new(data)?;
		if let Some(key) = scene.entity_ids().next().and_then(|id| scene.key(id)) {
			session.execute_command(EditorCommandRequest {
				expected_revision: session.revision(),
				command: Command::Select {
					keys: vec![key.as_str().to_owned()],
				},
			})?;
		}
		self.session = Some(session);
		self.capture_resources(scene)?;
		self.refresh_selection(scene);
		Ok(())
	}

	fn capture_resources(&mut self, scene: &Scene) -> Result<(), String> {
		for id in scene.entity_ids() {
			let key = scene
				.key(id)
				.ok_or("Native authoring entity needs a persistent key")?;
			if !self.resources.contains_key(key) {
				self.resources.insert(key.clone(), scene.snapshot(id)?);
			}
		}
		Ok(())
	}

	fn execute_core(&mut self, command: Command) -> Result<(), String> {
		let session = self
			.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?;
		session.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
	}

	fn apply(
		&mut self,
		scene: &mut Scene,
		command: Command,
		supplied: Option<&ModelInstance>,
	) -> Result<(), String> {
		self.initialize(scene)?;
		self.capture_resources(scene)?;
		self.execute_core(command)?;
		let document = self
			.session
			.as_ref()
			.expect("initialized session")
			.authored_document();
		scene.synchronize_retained(&document, &self.resources, supplied)?;
		self.capture_resources(scene)?;
		let retained = self
			.session
			.as_ref()
			.expect("initialized session")
			.retained_entity_keys();
		self.resources.retain(|key, _| retained.contains(key));
		self.refresh_selection(scene);
		Ok(())
	}

	fn refresh_selection(&mut self, scene: &Scene) {
		self.selected = self
			.session
			.as_ref()
			.and_then(|session| session.selection().first())
			.and_then(|key| scene.entity_by_key(key));
	}

	fn finish_gesture(&mut self) -> Result<(), String> {
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

	pub fn play_input_target(&self) -> &MovementTarget {
		&self.play_input_target
	}

	pub fn set_play_input_target(
		&mut self,
		target: MovementTarget,
		input: &mut InputState,
	) -> Result<bool, String> {
		let session = self
			.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?;

		let targets = session
			.play_controlled_targets()
			.ok_or("Play mode is not active")?;

		if let MovementTarget::Entity(entity) = &target
			&& !targets.contains(entity)
		{
			return Err("Input target has no configured controlled movement behavior".into());
		}

		if self.play_input_target == target {
			return Ok(false);
		}

		session.clear_play_input();
		input.suppress_held();
		self.play_input_target = target;

		Ok(true)
	}

	pub fn play_movement_held(&self, input: &InputState) -> bool {
		self.is_playing()
			&& !self.simulation_paused()
			&& self
				.play_controlled_targets()
				.is_some_and(|targets| !targets.is_empty())
			&& self.play_movement_bindings.any_held(input)
	}

	fn route_play_input(&mut self, movement: [f32; 3]) -> Result<(), String> {
		match self.play_input_target.clone() {
			MovementTarget::AllControlled => self.set_play_input(movement),
			MovementTarget::Entity(entity) => self.set_entity_play_input(&entity, movement),
		}
	}
}

pub fn movement_direction(input: &InputState) -> [f32; 2] {
	MovementBindings::WASD.direction(input)
}
