use super::{EditorSession, PrefabData};
use crate::{
	SceneData,
	runtime_lifecycle::{PreparedRuntimeSpawn, RuntimeInspectionDto, RuntimeSpawnRequest},
	world::WorldChange,
};

impl EditorSession {
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
