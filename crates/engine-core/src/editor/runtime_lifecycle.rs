use super::{EditorSession, PrefabData};
use crate::{
	EntityKey, SceneData,
	input::{GamepadSample, InputProfile, PhysicalGamepadSample},
	runtime_lifecycle::{
		PreparedRuntimeSpawn, RuntimeEntityHandle, RuntimeInspectionDto, RuntimeSpawnRequest,
	},
	simulation_session::LocalInputScope,
	world::WorldChange,
};

impl EditorSession {
	pub fn play_entity_handle(&self, key: &EntityKey) -> Option<RuntimeEntityHandle> {
		self.play.as_ref()?.active().runtime_handle(key)
	}

	pub fn play_possession_generation(&self) -> Option<u64> {
		Some(self.play.as_ref()?.active().local_possession_generation())
	}

	pub fn local_input_scope(&self) -> Option<LocalInputScope> {
		self.play.as_ref()?.active().local_input_scope()
	}

	/// Request the same fixed-step possession change available to gameplay modules.
	pub fn request_play_possession(
		&mut self,
		run_id: &str,
		scene_generation: u64,
		expected_generation: u64,
		target: Option<RuntimeEntityHandle>,
	) -> Result<(), String> {
		let play = self
			.play
			.as_mut()
			.ok_or("Start Play before transferring control")?;
		if play.run_id() != run_id || play.scene_generation() != scene_generation {
			return Err("Possession request belongs to an inactive run or scene".into());
		}
		play.active_mut()
			.request_local_possession(expected_generation, target)
	}

	pub fn submit_ordered_play_controls_from_codes(
		&mut self,
		scope: &LocalInputScope,
		sequence: u64,
		keys: &std::collections::HashSet<String>,
		physical: PhysicalGamepadSample,
		logical: GamepadSample,
	) -> Result<bool, String> {
		let active = self
			.play
			.as_mut()
			.ok_or("Play mode is not active")?
			.active_mut();
		if active.input_action_map().is_some() {
			return active.submit_local_action_codes(scope, sequence, keys, physical);
		}
		let defaults = InputProfile::default();
		let profile = self.project.input_profile.as_ref().unwrap_or(&defaults);
		active.submit_local_controls(
			scope,
			sequence,
			profile.resolve_controls_from_codes(keys, logical),
		)
	}

	pub fn has_runtime_instances(&self) -> bool {
		self.play
			.as_ref()
			.is_some_and(|play| play.active().has_runtime_instances())
	}
	pub fn pending_runtime_spawns(&self) -> Vec<RuntimeSpawnRequest> {
		self.play
			.as_ref()
			.map(|play| play.active().pending_runtime_spawns())
			.unwrap_or_default()
	}
	pub fn reject_runtime_spawn(&mut self, id: u64, error: &str) -> Result<(), String> {
		self.play
			.as_mut()
			.ok_or("Start Play before handling runtime spawns")?
			.active_mut()
			.reject_runtime_spawn(id, error);
		Ok(())
	}
	pub fn prepare_runtime_spawn(
		&self,
		prefab: &PrefabData,
		fingerprint: &str,
		position: [f32; 3],
		request: Option<u64>,
	) -> Result<PreparedRuntimeSpawn, String> {
		self.play
			.as_ref()
			.ok_or("Start Play before spawning runtime entities")?
			.active()
			.prepare_runtime_spawn(prefab, fingerprint, position, request)
	}
	pub fn runtime_spawn_document(
		&self,
		prepared: &PreparedRuntimeSpawn,
	) -> Result<SceneData, String> {
		self.play
			.as_ref()
			.ok_or("Play has stopped")?
			.active()
			.runtime_spawn_document(prepared)
	}
	pub fn commit_runtime_spawn(
		&mut self,
		prepared: PreparedRuntimeSpawn,
	) -> Result<WorldChange, String> {
		self.ensure_revision_capacity()?;
		let change = self
			.play
			.as_mut()
			.ok_or("Play has stopped")?
			.active_mut()
			.commit_runtime_spawn(prepared)?;
		self.scene_revision += 1;
		Ok(change)
	}
	pub fn runtime_inspection(
		&self,
		offset: usize,
		limit: usize,
	) -> Result<RuntimeInspectionDto, String> {
		let play = self
			.play
			.as_ref()
			.ok_or("Start Play to inspect the runtime")?;
		play.active()
			.runtime_inspection(play.play_session_id(), play.run_id(), offset, limit)
	}
}
