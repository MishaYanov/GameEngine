//! Compatibility adapter for the native development harness. Authoring and
//! undo/redo are delegated to the same CPU EditorSession used by Tauri.

use crate::{
	Camera, EntityKey,
	editor::{
		EditorSession,
		command::{EditorCommandDto as Command, EditorCommandRequest},
	},
	input::InputProfile,
	model_instance::ModelInstance,
	scene::{EntityId, RetainedEntity, Scene},
};
use std::collections::HashMap;

mod authoring;
mod play;
mod play_input;
pub use authoring::movement_direction;
pub use engine_core::game_session::PreparedScene;
pub use engine_core::project::{BehaviorSpace, BehaviorSpec, ProjectData};
pub use engine_core::simulation_session::{MovementTarget, SimulationSession, TickOutcome};

#[derive(Default)]
pub struct SceneEditor {
	session: Option<EditorSession>,
	resources: HashMap<EntityKey, RetainedEntity>,
	selected: Option<EntityId>,
	movement_entity: Option<EntityId>,
	play_input_target: MovementTarget,
	play_input_override: Option<InputProfile>,
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

	pub fn from_scene(scene: &Scene) -> Self {
		let mut editor = Self::default();
		// The legacy constructor is infallible. Invalid transient content is
		// reported by the first command instead of installing a second history.
		if editor.initialize(scene).is_ok() {
			editor.refresh_selection(scene);
		}
		editor
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
				self.resources.insert(key.clone(), scene.retain_entity(id)?);
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
}
