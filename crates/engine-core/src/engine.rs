//! Typed coordinator shared by editor hosts, players and headless tools.
//! It owns CPU state and events; filesystem and graphics adapters stay outside.
use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
	Bounds3, EntityKey,
	asset_registry::{AssetHandle, AssetMetadata, AssetRef, AssetRegistry, AssetRegistryLimits},
	command::{EditorCommandRequest, EditorSnapshotDto},
	editor::{CommandFactory, EditorSession, PrefabData},
	events::{BackendEventKind, ChangeJournal, ChangePage},
	project::ProjectData,
	scene_data::{EntityData, SceneData, validate_model_path},
	simulation_session::{ConfiguredBehaviorFactory, GameModuleFactory, TickOutcome},
	world::WorldChange,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendErrorCode {
	Conflict,
	InvalidCommand,
	CommandRejected,
	SimulationRejected,
	InvalidCursor,
	ResourceLimit,
	Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendError {
	pub code: BackendErrorCode,
	pub message: String,
	pub current_revision: Option<u64>,
}
impl fmt::Display for BackendError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.message)
	}
}
impl std::error::Error for BackendError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
	pub revision: u64,
	pub scene_revision: u64,
	pub change_cursor: u64,
	pub changes: WorldChange,
	pub state_changed: bool,
	pub project_changed: bool,
}

pub struct EngineBuilder {
	scene: SceneData,
	project: ProjectData,
	module_factory: Arc<dyn GameModuleFactory>,
	asset_limits: AssetRegistryLimits,
}
impl EngineBuilder {
	pub fn new(scene: SceneData, project: ProjectData) -> Self {
		Self {
			scene,
			project,
			module_factory: Arc::new(ConfiguredBehaviorFactory),
			asset_limits: AssetRegistryLimits::default(),
		}
	}
	pub fn module_factory(mut self, factory: Arc<dyn GameModuleFactory>) -> Self {
		self.module_factory = factory;
		self
	}
	pub fn asset_limits(mut self, limits: AssetRegistryLimits) -> Self {
		self.asset_limits = limits;
		self
	}
	pub fn build(self) -> Result<Engine, BackendError> {
		let invalid = |message| BackendError {
			code: BackendErrorCode::InvalidCommand,
			message,
			current_revision: None,
		};
		self.project.validate_scene(&self.scene).map_err(invalid)?;
		let assets = AssetRegistry::new(self.asset_limits).map_err(invalid)?;
		let mut session = EditorSession::for_project(self.scene, self.project).map_err(invalid)?;
		session
			.set_module_factory(self.module_factory)
			.map_err(invalid)?;
		Ok(Engine {
			session,
			assets,
			journal: ChangeJournal::default(),
		})
	}
}

pub struct Engine {
	session: EditorSession,
	assets: AssetRegistry,
	journal: ChangeJournal,
}
impl Engine {
	pub fn new(scene: SceneData, project: ProjectData) -> Result<Self, BackendError> {
		EngineBuilder::new(scene, project).build()
	}
	pub fn session(&self) -> &EditorSession {
		&self.session
	}
	pub fn assets(&self) -> &AssetRegistry {
		&self.assets
	}
	pub fn assets_mut(&mut self) -> &mut AssetRegistry {
		&mut self.assets
	}
	pub fn change_cursor(&self) -> u64 {
		self.journal.cursor()
	}
	pub fn retained_event_bytes(&self) -> usize {
		self.journal.retained_bytes()
	}
	pub fn snapshot(&self) -> Result<EditorSnapshotDto, BackendError> {
		self.session
			.snapshot()
			.map_err(|message| self.error(BackendErrorCode::Internal, message))
	}
	pub fn changes_since(&self, cursor: u64) -> Result<ChangePage, BackendError> {
		self.journal
			.since(cursor)
			.map_err(|message| self.error(BackendErrorCode::InvalidCursor, message))
	}

	pub fn execute(
		&mut self,
		request: EditorCommandRequest,
	) -> Result<CommandReceipt, BackendError> {
		if request.expected_revision != self.session.revision() {
			return Err(self.error(
				BackendErrorCode::Conflict,
				format!(
					"Revision conflict: expected {}, current {}",
					request.expected_revision,
					self.session.revision()
				),
			));
		}
		CommandFactory::validate(&request.command)
			.map_err(|message| self.error(BackendErrorCode::InvalidCommand, message))?;
		self.mutate_session(|session| session.execute_validated_command(request))
	}

	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, BackendError> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err(self.error(
				BackendErrorCode::InvalidCommand,
				"Elapsed time must be finite and nonnegative",
			));
		}
		self.ensure_event_capacity()?;
		let outcome = match self.session.tick(elapsed) {
			Ok(outcome) => outcome,
			Err(message) => {
				// Simulation rejects and rolls back an action batch, then pauses.
				// Consumers still need the state transition even on this error.
				self.publish(BackendEventKind::StateChanged)?;
				return Err(self.error(BackendErrorCode::SimulationRejected, message));
			}
		};
		if outcome.steps > 0 {
			self.publish(BackendEventKind::SimulationAdvanced {
				tick: outcome.tick,
				changes: outcome.changes.clone(),
			})?;
		}
		Ok(outcome)
	}

	pub fn replace_document(&mut self, data: SceneData) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.replace_document(data))
	}
	pub fn replace_project_document(
		&mut self,
		data: SceneData,
		project: ProjectData,
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.replace_project_document(data, project))
	}
	pub fn mark_saved(&mut self) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(EditorSession::mark_saved)
	}
	pub fn mark_scene_saved(&mut self) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(EditorSession::mark_scene_saved)
	}
	pub fn mark_project_saved(&mut self) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(EditorSession::mark_project_saved)
	}
	pub fn supersede_revision(&mut self, previous: u64) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.supersede_revision(previous))
	}
	pub fn clear_play_input(&mut self) {
		self.session.clear_play_input();
	}
	pub fn set_model_bounds(
		&mut self,
		path: String,
		mesh: usize,
		bounds: Bounds3,
	) -> Result<(), BackendError> {
		validate_model_path(&path)
			.map_err(|message| self.error(BackendErrorCode::InvalidCommand, message))?;
		if mesh > 1_000_000 {
			return Err(self.error(
				BackendErrorCode::InvalidCommand,
				"Asset mesh index exceeds the supported limit",
			));
		}
		self.session.set_model_bounds(path, mesh, bounds);
		Ok(())
	}
	pub fn replace_entities(
		&mut self,
		entities: Vec<EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.replace_entities_command(entities, selection, label))
	}
	/// Stage every metadata update before authoring can commit. An invalid
	/// scene/history change cannot leave a partially populated asset registry.
	pub fn replace_entities_with_assets(
		&mut self,
		entities: Vec<EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
		metadata: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<CommandReceipt, BackendError> {
		let (assets, _) = self
			.assets
			.staged_with_batch(metadata)
			.map_err(|message| self.error(BackendErrorCode::ResourceLimit, message))?;
		let receipt = self.replace_entities(entities, selection, label)?;
		self.assets = assets;
		self.refresh_asset_bounds();
		Ok(receipt)
	}
	pub fn register_assets(
		&mut self,
		metadata: Vec<(AssetRef, AssetMetadata)>,
	) -> Result<Vec<AssetHandle>, BackendError> {
		let handles = self
			.assets
			.register_batch(metadata)
			.map_err(|message| self.error(BackendErrorCode::ResourceLimit, message))?;
		self.refresh_asset_bounds();
		Ok(handles)
	}
	pub fn clear_assets(&mut self) {
		self.assets.clear();
		self.session.clear_model_bounds();
	}
	fn refresh_asset_bounds(&mut self) {
		self.session.clear_model_bounds();
		for (_, record) in self.assets.iter() {
			if let Some(bounds) = record.metadata.bounds {
				self.session.set_model_bounds(
					record.reference.path.clone(),
					record.reference.mesh_index.unwrap_or(0),
					bounds,
				);
			}
		}
	}
	pub fn instantiate_prefab(
		&mut self,
		prefab: PrefabData,
		position: [f32; 3],
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.instantiate_prefab_command(prefab, position))
	}
	pub fn export_prefab(&self) -> Result<PrefabData, BackendError> {
		self.session
			.export_prefab()
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))
	}

	fn mutate_session(
		&mut self,
		operation: impl FnOnce(&mut EditorSession) -> Result<(), String>,
	) -> Result<CommandReceipt, BackendError> {
		self.ensure_event_capacity()?;
		let revision = self.session.revision();
		let was_playing = self.session.is_playing();
		let before = self.session.authored_document();
		let before_project = self.session.project_document();
		operation(&mut self.session)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		let after = self.session.authored_document();
		let mut changes = if Arc::ptr_eq(&before, &after) {
			WorldChange::default()
		} else {
			WorldChange::between(&before, &after)
		};
		if was_playing != self.session.is_playing() {
			changes.full_resync = true;
			changes.changed_entities.clear();
			changes.removed_entities.clear();
		}
		let after_project = self.session.project_document();
		let project_changed =
			!Arc::ptr_eq(&before_project, &after_project) && before_project != after_project;
		let state_changed = revision != self.session.revision();
		if !changes.is_empty() {
			self.publish(BackendEventKind::DocumentChanged {
				changes: changes.clone(),
			})?;
		}
		if project_changed {
			self.publish(BackendEventKind::ProjectChanged)?;
		}
		if state_changed && changes.is_empty() && !project_changed {
			self.publish(BackendEventKind::StateChanged)?;
		}
		Ok(CommandReceipt {
			revision: self.session.revision(),
			scene_revision: self.session.scene_revision(),
			change_cursor: self.journal.cursor(),
			changes,
			state_changed,
			project_changed,
		})
	}
	fn ensure_event_capacity(&self) -> Result<(), BackendError> {
		// A compound scene/project command can publish two events. Check before
		// mutation so an exhausted journal cannot reject already-committed work.
		if self.journal.cursor().checked_add(2).is_none() {
			return Err(self.error(
				BackendErrorCode::ResourceLimit,
				"Backend event sequence is exhausted; reopen the engine",
			));
		}
		Ok(())
	}
	fn publish(&mut self, event: BackendEventKind) -> Result<(), BackendError> {
		self.journal
			.publish(
				self.session.revision(),
				self.session.scene_revision(),
				event,
			)
			.map_err(|message| self.error(BackendErrorCode::Internal, message))
	}
	fn error(&self, code: BackendErrorCode, message: impl Into<String>) -> BackendError {
		BackendError {
			code,
			message: message.into(),
			current_revision: Some(self.session.revision()),
		}
	}
}
