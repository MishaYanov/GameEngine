//! Typed coordinator shared by editor hosts, players and headless tools.
//! It owns CPU state and events; filesystem and graphics adapters stay outside.
mod assets;
mod play;
use std::{collections::HashSet, fmt, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
	Bounds3, EntityKey,
	asset_registry::{AssetHandle, AssetMetadata, AssetRef, AssetRegistry, AssetRegistryLimits},
	command::{EditorCommandRequest, EditorSnapshotDto, PlayStatusDto},
	editor::{CommandFactory, EditorSession, PrefabData},
	events::{BackendEventKind, ChangeJournal, ChangePage},
	game_session::PreparedScene,
	project::ProjectData,
	scene_catalog::SceneId,
	scene_data::{EntityData, SceneData, validate_model_path},
	simulation_session::{
		GameModuleFactory, GameModuleResolver, TickOutcome, resolve_default_gameplay,
	},
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
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub organization_changed: bool,
	pub active_scene_id: crate::scene_catalog::SceneId,
	pub affected_scenes: Vec<crate::scene_catalog::SceneId>,
}

pub struct EngineBuilder {
	documents: Vec<(crate::scene_catalog::SceneId, SceneData)>,
	scene: SceneData,
	project: ProjectData,
	module_resolver: GameModuleResolver,
	animation_resolver: Option<crate::animation::AnimationResolver>,
	asset_limits: AssetRegistryLimits,
}
impl EngineBuilder {
	pub fn new(scene: SceneData, project: ProjectData) -> Self {
		Self {
			documents: Vec::new(),
			scene,
			project,
			module_resolver: Arc::new(resolve_default_gameplay),
			animation_resolver: None,
			asset_limits: AssetRegistryLimits::default(),
		}
	}
	pub fn with_scenes(
		mut self,
		documents: Vec<(crate::scene_catalog::SceneId, SceneData)>,
	) -> Self {
		self.documents = documents;
		self
	}
	pub fn module_factory(self, factory: Arc<dyn GameModuleFactory>) -> Self {
		self.module_resolver(Arc::new(move |_| Ok(Arc::clone(&factory))))
	}
	pub fn module_resolver(mut self, resolver: GameModuleResolver) -> Self {
		self.module_resolver = resolver;
		self
	}
	pub fn asset_limits(mut self, limits: AssetRegistryLimits) -> Self {
		self.asset_limits = limits;
		self
	}
	pub fn animation_resolver(mut self, resolver: crate::animation::AnimationResolver) -> Self {
		self.animation_resolver = Some(resolver);
		self
	}
	pub fn build(self) -> Result<Engine, BackendError> {
		let invalid = |phase: &'static str| {
			move |message| {
				engine_logger::debug!(target: "engine_core::engine", "Engine build rejected: phase={phase}");
				BackendError {
					code: BackendErrorCode::InvalidCommand,
					message,
					current_revision: None,
				}
			}
		};
		self.project
			.validate_scene(&self.scene)
			.map_err(invalid("validate_scene"))?;
		let assets = AssetRegistry::new(self.asset_limits).map_err(invalid("asset_registry"))?;
		let mut session = EditorSession::for_project(self.scene, self.project)
			.map_err(invalid("editor_session"))?;
		session
			.load_saved_scenes(self.documents)
			.map_err(invalid("load_scenes"))?;
		session
			.set_module_resolver(self.module_resolver)
			.map_err(invalid("gameplay_module"))?;
		if let Some(resolver) = self.animation_resolver {
			session
				.set_animation_resolver(resolver)
				.map_err(invalid("animation_resolver"))?;
		}
		engine_logger::debug!(target: "engine_core::engine", "Engine initialized: entities={}, fixed_hz={}", session.authored_document().entities.len(), session.project().fixed_hz);
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
			engine_logger::debug!(target: "engine_core::editor", "Command rejected: code=conflict, expected_revision={}, current_revision={}", request.expected_revision, self.session.revision());
			return Err(self.error(
				BackendErrorCode::Conflict,
				format!(
					"Revision conflict: expected {}, current {}",
					request.expected_revision,
					self.session.revision()
				),
			));
		}
		let result = CommandFactory::validate(&request.command)
			.map_err(|message| self.error(BackendErrorCode::InvalidCommand, message))
			.and_then(|()| {
				self.mutate_session(|session| session.execute_validated_command(request))
			});
		result.inspect_err(|error| {
			engine_logger::debug!(target: "engine_core::editor", "Command failed: code={:?}, revision={}", error.code, self.session.revision());
		})
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
	pub fn mark_scene_saved_by_id(
		&mut self,
		id: &crate::scene_catalog::SceneId,
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|s| s.mark_scene_saved_by_id(id))
	}
	pub fn mark_project_saved(&mut self) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(EditorSession::mark_project_saved)
	}
	pub fn supersede_revision(&mut self, previous: u64) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.supersede_revision(previous))
	}
	pub fn replace_entities(
		&mut self,
		entities: Vec<EntityData>,
		selection: Vec<EntityKey>,
		label: &str,
	) -> Result<CommandReceipt, BackendError> {
		self.mutate_session(|session| session.replace_entities_command(entities, selection, label))
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
		let before_scene = self.session.active_scene_id().clone();
		let before_states = self.session.scene_states();
		let revision = self.session.revision();
		let was_playing = self.session.is_playing();
		let before = self.session.authored_document();
		let before_project = self.session.project_document();
		operation(&mut self.session)
			.map_err(|message| self.error(BackendErrorCode::CommandRejected, message))?;
		let after = self.session.authored_document();
		let organization_changed =
			!Arc::ptr_eq(&before, &after) && before.organization != after.organization;
		let mut changes = if Arc::ptr_eq(&before, &after) {
			WorldChange::default()
		} else {
			WorldChange::between(&before, &after)
		};
		if was_playing != self.session.is_playing()
			|| before_scene != *self.session.active_scene_id()
		{
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
		let after_states = self.session.scene_states();
		let mut affected_scenes: Vec<_> = before_states
			.iter()
			.chain(after_states.iter())
			.filter(|state| {
				before_states
					.iter()
					.find(|s| s.id == state.id)
					.map(|s| (&s.asset_path, s.dirty, s.loaded))
					!= after_states
						.iter()
						.find(|s| s.id == state.id)
						.map(|s| (&s.asset_path, s.dirty, s.loaded))
			})
			.map(|s| s.id.clone())
			.collect();
		if organization_changed || !changes.is_empty() {
			affected_scenes.push(self.session.active_scene_id().clone());
		}
		if project_changed {
			let definitions_changed =
				before_project.behavior_definitions != after_project.behavior_definitions;
			for state in before_states.iter().chain(after_states.iter()) {
				// Shared configuration changes affect referencing scenes even when
				// their authored attachments are unchanged. This is deliberately
				// conservative and cannot fail after the command has committed.
				let shared_configuration_changed = definitions_changed
					&& before_project
						.behaviors_for(&state.id)
						.chain(after_project.behaviors_for(&state.id))
						.any(|behavior| {
							matches!(behavior, crate::project::BehaviorSpec::Shared { .. })
						});
				if !before_project
					.behaviors_for(&state.id)
					.eq(after_project.behaviors_for(&state.id))
					|| shared_configuration_changed
				{
					affected_scenes.push(state.id.clone());
				}
			}
			if before_project.startup_scene_id != after_project.startup_scene_id {
				affected_scenes.push(before_project.startup_scene_id.clone());
				affected_scenes.push(after_project.startup_scene_id.clone());
			}
		}
		affected_scenes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
		affected_scenes.dedup();
		Ok(CommandReceipt {
			active_scene_id: self.session.active_scene_id().clone(),
			affected_scenes,
			revision: self.session.revision(),
			scene_revision: self.session.scene_revision(),
			change_cursor: self.journal.cursor(),
			changes,
			state_changed,
			project_changed,
			organization_changed,
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
		self.publish_for_scene(self.session.active_scene_id().clone(), event)
	}
	fn publish_for_scene(
		&mut self,
		scene_id: SceneId,
		event: BackendEventKind,
	) -> Result<(), BackendError> {
		self.journal
			.publish_for_scene(
				self.session.revision(),
				self.session.scene_revision(),
				event,
				Some(scene_id),
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
