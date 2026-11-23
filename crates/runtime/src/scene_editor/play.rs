//! Play setup, scene installation, ticking, and runtime prefab requests.

use super::{MovementTarget, PreparedScene, SceneEditor, TickOutcome};
use crate::{
	Camera, EntityKey, InputState, Key, SceneData,
	editor::{EditorSession, command::EditorCommandDto as Command},
	input::{GamepadSample, PhysicalGamepadSample},
	scene::Scene,
};
use engine_core::{
	game_progress::{GameProgress, ProgressAction},
	scene_catalog::SceneId,
	simulation_session::{GameModuleFactory, GameModuleResolver},
};
use std::sync::Arc;

impl SceneEditor {
	pub fn is_playing(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::is_playing)
	}
	pub fn set_module_factory(
		&mut self,
		factory: Arc<dyn GameModuleFactory>,
	) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?
			.set_module_factory(factory)
	}
	pub fn set_module_resolver(&mut self, resolver: GameModuleResolver) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?
			.set_module_resolver(resolver)
	}
	pub fn play_progress<P: GameProgress>(&self) -> Option<&P> {
		self.session.as_ref()?.play_progress::<P>()
	}
	pub fn play_status(&self) -> Option<engine_core::command::PlayStatusDto> {
		self.session.as_ref()?.play_status()
	}
	pub fn render_camera(&self) -> Option<Camera> {
		self.session.as_ref().map(EditorSession::render_camera)
	}

	pub fn camera_replay_state(
		&self,
	) -> Result<Option<crate::camera_replay::CameraReplayProjection>, String> {
		match &self.session {
			Some(session) => session.camera_replay_state(),
			None => Ok(None),
		}
	}
	/// A lost orbit target still owns its last accepted pose until Play changes scene.
	pub fn play_rig_camera(&self) -> Option<Camera> {
		let session = self.session.as_ref()?;
		session
			.play_has_game_camera_owner()
			.then(|| session.render_camera())
	}
	pub fn apply_play_progress(&mut self, actions: &[ProgressAction]) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?
			.apply_play_progress(actions)
	}
	pub fn prepare_new_game(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		self.session
			.as_ref()
			.ok_or("Native authoring session is not initialized")?
			.prepare_new_game(scene_id, document)
	}

	pub fn prepare_restored_game<P: GameProgress>(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
		progress: P,
	) -> Result<PreparedScene, String> {
		self.session
			.as_ref()
			.ok_or("Native authoring session is not initialized")?
			.prepare_restored_game(scene_id, document, progress)
	}

	pub fn play_particles(
		&self,
	) -> impl Iterator<Item = engine_core::particles::ParticleSample> + '_ {
		self.session.iter().flat_map(EditorSession::particles)
	}

	pub fn play_world(&self) -> Option<&engine_core::world::RuntimeWorld> {
		self.session.as_ref()?.render_world()
	}

	pub fn play_document(&self) -> Option<Arc<SceneData>> {
		let session = self.session.as_ref()?;
		session.is_playing().then(|| session.render_document())
	}

	pub fn play_controlled_targets(&self) -> Option<&[EntityKey]> {
		self.session
			.as_ref()
			.and_then(EditorSession::play_controlled_targets)
	}

	pub fn play_scene_id(&self) -> Option<&SceneId> {
		self.session.as_ref().and_then(EditorSession::play_scene_id)
	}

	pub fn pending_play_scene(&self) -> Option<&SceneId> {
		self.session
			.as_ref()
			.and_then(EditorSession::pending_play_scene)
	}

	pub fn prepare_play_scene(
		&self,
		scene_id: SceneId,
		document: Arc<SceneData>,
	) -> Result<PreparedScene, String> {
		self.session
			.as_ref()
			.ok_or("Native authoring session is not initialized")?
			.prepare_play_scene(scene_id, document)
	}

	/// Installation boundary. The caller must finish GPU work before releasing
	/// retained snapshots and replace the rendered scene after success.
	pub fn commit_play_scene(
		&mut self,
		prepared: PreparedScene,
		input: &mut InputState,
		retain_authored_resources: bool,
	) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native authoring session is not initialized")?
			.commit_play_scene(prepared)?;
		self.play_input_target = MovementTarget::AllControlled;
		self.selected = None;
		self.movement_entity = None;
		if !retain_authored_resources {
			self.resources.clear();
		}
		input.suppress_held();
		Ok(())
	}

	pub fn simulation_paused(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::is_paused)
	}

	pub fn animations(&self) -> Option<&engine_core::animation::AnimationBank> {
		self.session.as_ref().and_then(EditorSession::animations)
	}
	pub fn needs_tick(&self) -> bool {
		self.session.as_ref().is_some_and(EditorSession::needs_tick)
	}

	pub fn start_play(&mut self, scene: &mut Scene) -> Result<(), String> {
		self.initialize(scene)?;
		self.finish_gesture()?;
		if let Some(root) = scene.animation_root().map(std::path::Path::to_path_buf) {
			let input_root = root.clone();
			let personal_inputs = self.personal_input_layouts.clone();
			let controls_root = root.clone();
			let animator_root = root.clone();
			self.session
				.as_mut()
				.expect("initialized session")
				.set_animator_resolver(Arc::new(move |project, scene_id, scene| {
					bridge::animator_graph::prepare_scene(&animator_root, project, scene_id, scene)
				}))?;

			self.session
				.as_mut()
				.expect("initialized session")
				.set_input_actions_resolver(Arc::new(move |data| {
					let authored = bridge::input_actions::load_scene(&input_root, data)?;
					let (Some(map), Some(id), Some(read)) = (
						authored.as_ref(),
						bridge::input_actions::scene_asset_id(data),
						&personal_inputs,
					) else {
						return Ok(authored);
					};
					match read(id).and_then(|layout| layout.map(|layout| map.with_layout(id, &layout)).transpose()) {
						Ok(Some(effective)) => Ok(Some(effective)),
						Ok(None) => Ok(authored),
						Err(problem) => {
							engine_logger::warn!(target: "runtime::play_input", "Player keyboard uses authored bindings: {problem}");
							Ok(authored)
						}
					}
				}))?;
			self.session
				.as_mut()
				.expect("initialized session")
				.set_control_graph_resolver(Arc::new(move |project, data, map| {
					bridge::controls_graph::prepare_scene(&controls_root, project, data, map)
				}))?;
			self.session
				.as_mut()
				.unwrap()
				.set_animation_resolver(Arc::new(move |data| {
					bridge::animation_asset::prepare_scene(&root, data)
				}))?;
		}
		self.execute_core(Command::StartPlay)?;
		self.play_input_target = MovementTarget::default();
		let session = self.session.as_ref().expect("initialized session");
		scene.synchronize(&session.render_document())?;
		if let Some(bank) = session.animations() {
			scene.synchronize_animation(bank)?;
		}
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
		scene.restore_authored_animation();
		self.refresh_selection(scene);
		Ok(())
	}

	pub fn update_play(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		elapsed: f32,
	) -> Result<TickOutcome, String> {
		self.update_play_with_gamepad(scene, input, GamepadSample::default(), elapsed)
	}

	pub fn update_play_with_gamepad(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		gamepad: GamepadSample,
		elapsed: f32,
	) -> Result<TickOutcome, String> {
		self.update_play_with_action_controls(
			scene,
			input,
			gamepad,
			PhysicalGamepadSample::default(),
			elapsed,
		)
	}

	pub fn update_play_with_action_controls(
		&mut self,
		scene: &mut Scene,
		input: &InputState,
		mut gamepad: GamepadSample,
		mut physical: PhysicalGamepadSample,
		elapsed: f32,
	) -> Result<TickOutcome, String> {
		self.initialize(scene)?;
		if input.just_pressed(Key::Space) && !self.play_input_binds_key(Key::Space) {
			self.execute_core(Command::SetPaused {
				paused: !self.simulation_paused(),
			})?;
			gamepad = GamepadSample::default();
			physical = PhysicalGamepadSample::default();
		}
		self.sample_play_action_controls(input, gamepad, physical)?;
		let session = self.session.as_mut().expect("initialized session");
		if !session.has_play_visibility_capabilities() {
			session.set_play_visibility_capabilities(scene.visibility_capabilities())?;
		}
		let outcome = session.tick(elapsed as f64)?;
		Self::synchronize_play_tick(scene, session, &outcome)?;
		Ok(outcome)
	}

	/// Drain accepted fixed steps after the host fulfills prefab requests. Input
	/// edges were consumed by update_play and must not be applied a second time.
	pub fn drain_play(&mut self, scene: &mut Scene) -> Result<TickOutcome, String> {
		let session = self
			.session
			.as_mut()
			.ok_or("Native session is unavailable")?;
		let outcome = session.tick(0.0)?;
		Self::synchronize_play_tick(scene, session, &outcome)?;
		Ok(outcome)
	}

	fn synchronize_play_tick(
		scene: &mut Scene,
		session: &EditorSession,
		outcome: &TickOutcome,
	) -> Result<(), String> {
		if !outcome.changes.is_empty()
			&& let Some(world) = session.render_world()
			&& !scene.synchronize_world(world, &outcome.changes)?
		{
			return Err(
				"Configured simulation changed GPU topology; reload through the asset factory"
					.into(),
			);
		}
		if let Some(bank) = session.animations() {
			scene.synchronize_animation(bank)?;
		}
		Ok(())
	}

	pub fn has_runtime_instances(&self) -> bool {
		self.session
			.as_ref()
			.is_some_and(EditorSession::has_runtime_instances)
	}
	pub fn pending_runtime_spawns(
		&self,
	) -> Vec<engine_core::runtime_lifecycle::RuntimeSpawnRequest> {
		self.session
			.as_ref()
			.map(EditorSession::pending_runtime_spawns)
			.unwrap_or_default()
	}
	pub fn reject_runtime_spawn(&mut self, id: u64, error: &str) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native session is unavailable")?
			.reject_runtime_spawn(id, error)
	}
	pub fn prepare_runtime_spawn(
		&self,
		prefab: &engine_core::editor::PrefabData,
		fingerprint: &str,
		position: [f32; 3],
		request: Option<u64>,
	) -> Result<engine_core::runtime_lifecycle::PreparedRuntimeSpawn, String> {
		self.session
			.as_ref()
			.ok_or("Native session is unavailable")?
			.prepare_runtime_spawn(prefab, fingerprint, position, request)
	}
	pub fn runtime_spawn_document(
		&self,
		prepared: &engine_core::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<SceneData, String> {
		self.session
			.as_ref()
			.ok_or("Native session is unavailable")?
			.runtime_spawn_document(prepared)
	}
	pub fn commit_runtime_spawn(
		&mut self,
		prepared: engine_core::runtime_lifecycle::PreparedRuntimeSpawn,
	) -> Result<(), String> {
		self.session
			.as_mut()
			.ok_or("Native session is unavailable")?
			.commit_runtime_spawn(prepared)?;
		Ok(())
	}
}
