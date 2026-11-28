use super::EditorSession;
use crate::{
	SceneData,
	scene_catalog::SceneId,
	simulation_session::{PreparedRegionChange, SimulationSession, WorldRegionState},
	world::WorldChange,
};
use std::sync::Arc;

impl EditorSession {
	pub fn resident_regions(&self) -> Vec<SceneId> {
		self.play
			.as_ref()
			.map(|play| play.active().resident_regions())
			.unwrap_or_default()
	}

	pub fn streaming_focus(&self) -> Option<[f32; 3]> {
		self.play.as_ref()?.active().streaming_focus()
	}

	pub fn can_detach_region(&self, scene_id: &SceneId) -> bool {
		self.region_session()
			.is_ok_and(|session| session.can_detach_region(scene_id))
	}

	pub fn prepare_region_attach(
		&self,
		scene_id: &SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedRegionChange, String> {
		self.region_session()?
			.prepare_region_attach(scene_id, document)
	}

	pub fn prepare_region_detach(
		&self,
		scene_id: &SceneId,
	) -> Result<PreparedRegionChange, String> {
		self.region_session()?.prepare_region_detach(scene_id)
	}

	pub fn commit_region_change(
		&mut self,
		prepared: PreparedRegionChange,
	) -> Result<WorldChange, String> {
		self.region_session()?;
		self.ensure_revision_capacity()?;
		let change = self
			.play
			.as_mut()
			.expect("validated Play session")
			.active_mut()
			.commit_region_change(prepared)?;
		self.scene_revision += 1;
		Ok(change)
	}

	pub fn capture_region_state(&self) -> Result<WorldRegionState, String> {
		self.region_session()?.capture_region_state()
	}

	pub fn restore_region_state(&mut self, state: WorldRegionState) -> Result<(), String> {
		self.region_session()?;
		self.play
			.as_mut()
			.expect("validated Play session")
			.active_mut()
			.restore_region_state(state)
	}

	fn region_session(&self) -> Result<&SimulationSession, String> {
		self.require_no_transaction()?;
		let play = self
			.play
			.as_ref()
			.ok_or("Start Play before streaming a world region")?;
		if play.pending_transition().is_some() {
			return Err("A full scene transition is pending".into());
		}
		Ok(play.active())
	}
}
