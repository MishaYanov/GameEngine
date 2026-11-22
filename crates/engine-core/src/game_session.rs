use crate::camera_input::{CameraInputOwner, CameraIntent};
use crate::game_progress::{GameProgress, ProgressAction, ProgressState};
use crate::project::ProjectData;
use crate::scene_catalog::SceneId;
use crate::simulation_session::{GameModuleFactory, SimulationSession, TickOutcome};
use crate::{SceneData, command::PlayStatusDto};
use std::sync::Arc;

pub struct GameSession {
	animation_resolver: Option<crate::animation::AnimationResolver>,
	project: Arc<ProjectData>,
	factory: Arc<dyn GameModuleFactory>,
	active: SimulationSession,
	play_session_id: String,
	identity: Arc<String>,
	progress: ProgressState,
	scene_generation: u64,
	pending_scene: Option<SceneId>,
}

pub struct PreparedScene {
	new_run: Option<(ProgressState, Arc<String>)>,
	owner: Arc<String>,
	expected_generation: u64,
	restore_guard: Option<(u64, u64)>,
	simulation: SimulationSession,
}

impl PreparedScene {
	pub fn scene_id(&self) -> &SceneId {
		self.simulation.scene_id()
	}
}

impl GameSession {
	pub fn set_animation_resolver(
		&mut self,
		resolver: crate::animation::AnimationResolver,
	) -> Result<(), String> {
		self.active.set_animation_resolver(Arc::clone(&resolver))?;
		self.animation_resolver = Some(resolver);
		Ok(())
	}
	pub fn play_session_id(&self) -> &str {
		&self.play_session_id
	}

	pub fn run_id(&self) -> &str {
		&self.identity
	}

	pub fn status(&self) -> PlayStatusDto {
		PlayStatusDto {
			play_session_id: self.play_session_id.clone(),
			run_id: self.run_id().to_owned(),
			scene_generation: self.scene_generation,
			scene_id: self.active.scene_id().clone(),
			paused: self.active.paused(),
			pending_scene_id: self.pending_scene.clone(),
			gameplay: self.active.gameplay_status(self.progress.view()),
		}
	}

	pub fn progress<P: GameProgress>(&self) -> Option<&P> {
		self.progress.view().get::<P>()
	}
	pub fn progress_revision(&self) -> u64 {
		self.progress.view().revision()
	}
	pub fn apply_progress_actions(&mut self, actions: &[ProgressAction]) -> Result<bool, String> {
		self.progress.apply_batch(actions)
	}
	pub fn tick(&mut self, elapsed: f64) -> Result<TickOutcome, String> {
		let outcome = self.active.tick_with_progress(
			elapsed,
			&mut self.progress,
			self.pending_scene.is_some(),
		)?;
		if let Some(scene_id) = &outcome.requested_scene {
			self.pending_scene = Some(scene_id.clone());
			engine_logger::debug!(target: "engine_core::play", "Runtime scene transition requested: tick={}, generation={}", outcome.tick, self.scene_generation);
		}
		Ok(outcome)
	}
	pub fn needs_tick(&self) -> bool {
		self.active.needs_tick_with_progress(self.progress.view())
	}
	pub fn active(&self) -> &SimulationSession {
		&self.active
	}
	pub fn camera_replay_state(
		&self,
	) -> Result<crate::camera_replay::CameraReplayProjection, String> {
		self.active.camera_replay_state()
	}
	pub fn active_mut(&mut self) -> &mut SimulationSession {
		&mut self.active
	}
	pub fn scene_generation(&self) -> u64 {
		self.scene_generation
	}

	/// The outstanding request stays available if a host cannot prepare its destination.
	pub fn pending_scene(&self) -> Option<&SceneId> {
		self.pending_scene.as_ref()
	}

	pub fn prepare_pending_scene(&self, document: Arc<SceneData>) -> Result<PreparedScene, String> {
		let scene_id = self
			.pending_scene()
			.ok_or("No runtime scene request is pending")?;
		self.prepare_scene(scene_id.clone(), document)
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
		if let Some(resolver) = &self.animation_resolver {
			simulation.set_animation_resolver(Arc::clone(resolver))?;
		}
		simulation.require_animation_ready()?;

		Ok(PreparedScene {
			new_run: None,
			owner: Arc::clone(&self.identity),
			expected_generation: self.scene_generation,
			restore_guard: None,
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
			animation_resolver: None,
			project,
			factory,
			active,
			play_session_id: uuid::Uuid::new_v4().to_string(),
			identity: Arc::new(uuid::Uuid::new_v4().to_string()),
			progress,
			scene_generation: 0,
			pending_scene: None,
		})
	}

	pub fn prepare_new_game(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		self.prepare_replacement_run(scene_id, document, None)
	}

	/// Rebuild transient state around module-validated durable progress. The
	/// returned candidate belongs to this exact run, generation, and tick.
	pub fn prepare_restored_game<P: GameProgress>(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
		progress: P,
	) -> Result<PreparedScene, String> {
		if self.progress::<P>().is_none() {
			return Err("Restored progress does not match the active game module".into());
		}
		self.prepare_replacement_run(scene_id, document, Some(ProgressState::new(progress)?))
	}

	fn prepare_replacement_run(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
		restored_progress: Option<ProgressState>,
	) -> Result<PreparedScene, String> {
		let mut next = Self::with_factory_for_scene(
			Arc::clone(&self.project),
			scene_id,
			document,
			Arc::clone(&self.factory),
		)?;
		if let Some(resolver) = &self.animation_resolver {
			next.set_animation_resolver(Arc::clone(resolver))?;
		}
		next.active.require_animation_ready()?;
		let restore_guard = restored_progress
			.as_ref()
			.map(|_| (self.active.tick_index(), self.progress_revision()));
		Ok(PreparedScene {
			owner: Arc::clone(&self.identity),
			expected_generation: self.scene_generation,
			restore_guard,
			simulation: next.active,
			new_run: Some((restored_progress.unwrap_or(next.progress), next.identity)),
		})
	}

	/// Capture an owner once when a native viewport grabs the pointer.
	pub fn camera_input_owner(&self) -> Option<CameraInputOwner> {
		self.camera_input_active().then(|| CameraInputOwner {
			run_id: self.run_id().to_owned(),
			scene_generation: self.scene_generation,
		})
	}

	pub fn owns_camera_input(&self, owner: &CameraInputOwner) -> bool {
		self.camera_input_active()
			&& self.scene_generation == owner.scene_generation
			&& self.run_id() == owner.run_id
	}

	fn camera_input_active(&self) -> bool {
		!self.active.paused()
			&& self
				.active
				.orbit_state()
				.is_some_and(|rig| !rig.target_lost)
	}

	/// Switch a running presentation camera without modifying its authored scene.
	pub fn set_runtime_camera(
		&mut self,
		run_id: &str,
		scene_generation: u64,
		target: Option<crate::runtime_lifecycle::RuntimeEntityHandle>,
	) -> Result<(), String> {
		if self.run_id() != run_id || self.scene_generation() != scene_generation {
			return Err("Camera switch belongs to an inactive run or scene".into());
		}
		self.active_mut().set_runtime_camera(target)
	}

	pub fn submit_camera_input(
		&mut self,
		run_id: &str,
		scene_generation: u64,
		intent: CameraIntent,
	) -> Result<(), String> {
		if self.run_id() != run_id || self.scene_generation() != scene_generation {
			return Err("Camera input belongs to an inactive run or scene".into());
		}
		self.active_mut().submit_camera_input(intent)
	}

	pub fn commit_scene(&mut self, mut prepared: PreparedScene) -> Result<(), String> {
		if !Arc::ptr_eq(&self.identity, &prepared.owner) {
			return Err("Prepared scene belongs to another game session".into());
		}
		if prepared.expected_generation != self.scene_generation {
			return Err("Scene generation counter is out of sync".into());
		}
		if prepared
			.restore_guard
			.is_some_and(|guard| guard != (self.active.tick_index(), self.progress_revision()))
		{
			return Err("Progress advanced while Continue was being prepared".into());
		}

		let restarted = prepared.new_run.is_some();
		let next_generation = if restarted {
			0
		} else {
			self.scene_generation
				.checked_add(1)
				.ok_or_else(|| "Scene generation counter is exhausted".to_string())?
		};

		if !restarted {
			prepared.simulation.set_paused(self.active.paused());
		}
		prepared.simulation.clear_input();
		if let Some((progress, identity)) = prepared.new_run {
			self.progress = progress;
			self.identity = identity;
		}

		self.active = prepared.simulation;
		self.scene_generation = next_generation;
		self.pending_scene = None;
		engine_logger::info!(target: "engine_core::play", "Runtime scene committed: restarted={restarted}, generation={next_generation}");

		Ok(())
	}
}
