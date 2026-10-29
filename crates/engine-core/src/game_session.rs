use crate::SceneData;
use crate::game_progress::{GameProgress, ProgressAction, ProgressState};
use crate::project::ProjectData;
use crate::scene_catalog::SceneId;
use crate::simulation_session::{GameModuleFactory, SimulationSession};
use std::sync::Arc;

pub struct GameSession {
	project: Arc<ProjectData>,
	factory: Arc<dyn GameModuleFactory>,
	active: SimulationSession,
	identity: Arc<()>,
	progress: ProgressState,
	scene_generation: u64,
}

pub struct PreparedScene {
	new_progress: Option<ProgressState>,
	owner: Arc<()>,
	expected_generation: u64,
	simulation: SimulationSession,
}

impl GameSession {
	pub fn progress<P: GameProgress>(&self) -> Option<&P> {
		self.progress.view().get::<P>()
	}
	pub fn progress_revision(&self) -> u64 {
		self.progress.view().revision()
	}
	pub fn apply_progress_actions(&mut self, actions: &[ProgressAction]) -> Result<bool, String> {
		self.progress.apply_batch(actions)
	}
	pub fn tick(&mut self, elapsed: f64) -> Result<crate::simulation_session::TickOutcome, String> {
		self.active.tick_with_progress(elapsed, &mut self.progress)
	}
	pub fn needs_tick(&self) -> bool {
		self.active.needs_tick_with_progress(self.progress.view())
	}
	pub fn active(&self) -> &SimulationSession {
		&self.active
	}
	pub fn active_mut(&mut self) -> &mut SimulationSession {
		&mut self.active
	}
	pub fn scene_generation(&self) -> u64 {
		self.scene_generation
	}

	pub fn prepare_scene(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		if self.active.scene_id() == &scene_id {
			return Err("Cannot prepare active scene".into());
		}

		self.scene_generation
			.checked_add(1)
			.ok_or_else(|| "Scene generation counter is exhausted".to_string())?;

		let mut simulation = SimulationSession::with_shared_project_for_scene(
			Arc::clone(&self.project),
			scene_id,
			document,
			self.factory.as_ref(),
		)?;

		simulation.set_paused(self.active.paused());

		Ok(PreparedScene {
			new_progress: None,
			owner: Arc::clone(&self.identity),
			expected_generation: self.scene_generation,
			simulation,
		})
	}

	pub fn with_factory_for_scene(
		project: Arc<ProjectData>,
		scene_id: SceneId,
		document: Arc<SceneData>,
		factory: Arc<dyn GameModuleFactory>,
	) -> Result<Self, String> {
		let progress = factory.create_progress()?;
		let active = SimulationSession::with_shared_project_for_scene(
			Arc::clone(&project),
			scene_id,
			document,
			factory.as_ref(),
		)?;

		Ok(Self {
			project,
			factory,
			active,
			identity: Arc::new(()),
			progress,
			scene_generation: 0,
		})
	}

	pub fn prepare_new_game(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		let next = Self::with_factory_for_scene(
			Arc::clone(&self.project),
			scene_id,
			document,
			Arc::clone(&self.factory),
		)?;
		Ok(PreparedScene {
			owner: Arc::clone(&self.identity),
			expected_generation: self.scene_generation,
			simulation: next.active,
			new_progress: Some(next.progress),
		})
	}

	pub fn commit_scene(&mut self, mut prepared: PreparedScene) -> Result<(), String> {
		if !Arc::ptr_eq(&self.identity, &prepared.owner) {
			return Err("Prepared scene belongs to another game session".into());
		}
		if prepared.expected_generation != self.scene_generation {
			return Err("Scene generation counter is out of sync".into());
		}

		let next_generation = if prepared.new_progress.is_some() {
			0
		} else {
			self.scene_generation
				.checked_add(1)
				.ok_or_else(|| "Scene generation counter is exhausted".to_string())?
		};

		if prepared.new_progress.is_none() {
			prepared.simulation.set_paused(self.active.paused());
		}
		prepared.simulation.clear_input();
		if let Some(progress) = prepared.new_progress {
			self.progress = progress;
			self.identity = Arc::new(());
		}

		self.active = prepared.simulation;
		self.scene_generation = next_generation;

		Ok(())
	}
}
