mod editor_panel;
mod editor_tools;
mod game;
mod gizmo_overlay;
mod native_input;
mod selection_overlay;

use courtyard_gameplay as progress;
use editor_panel::PanelView;
use editor_tools::EditorTools;
use runtime::FrameClock;
use runtime::scene::Scene;
use runtime::{Camera, CameraController};
use runtime::{InputState, Key};
use runtime::{SceneEditor, Transform, model_instance::ModelInstance};
use selection_overlay::SelectionOverlay;

use ::game::launch::{LaunchMode, LaunchOptions};
use engine_logger::ResultLogExt;
use renderer::{WindowRenderer, vulkan::CameraUniform};
use runtime::assets::AssetManager;
use runtime::camera_input::{
	CameraInputOwner, CameraIntent, WheelSample, normalize_mouse, normalize_wheel,
};
use runtime::input::{InputProfile, MAX_INPUT_PROFILE_BYTES, MovementBindings};
use runtime::scene_catalog::SceneId;
use runtime::scene_editor::ProjectData;
use runtime::scene_io;
use std::{
	fs::File,
	io::Read,
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant},
};
use winit::{
	application::ApplicationHandler,
	event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::PhysicalKey,
	window::{CursorGrabMode, Window, WindowId},
};

#[derive(Clone, Copy)]
enum SceneCommand {
	Reload,
	Save,
	TogglePlay,
	DeleteSelected,
	DuplicateSelected,
	SpawnRobot,
}

struct AuthoredScene {
	scene: Scene,
	camera: Camera,
}

#[derive(Default)]
struct GameApp {
	// These external GPU resources must be released before the renderer/device.
	scene: Scene,
	authored: Option<AuthoredScene>,
	editor: SceneEditor,
	tools: EditorTools,
	selection_overlay: Option<SelectionOverlay>,
	game_hud: Option<renderer::ui::ScreenOverlay>,
	spawn_template: Option<ModelInstance>,
	asset_manager: Option<AssetManager>,
	renderer: Option<WindowRenderer>,
	window: Option<Arc<Window>>,
	clock: FrameClock,
	input: InputState,
	camera: Camera,
	camera_controller: CameraController,
	mouse_captured: bool,
	orbit_capture: Option<CameraInputOwner>,
	last_heartbeat: Option<Instant>,
	project_name: Option<String>,
	asset_root: Option<PathBuf>,
	scene_path: Option<PathBuf>,
	cursor_position: Option<[f64; 2]>,
	last_window_title: String,
	// Default matches cargo run Development. Player mode shares the configured
	// core simulation and skips the transitional native authoring harness.
	standalone_runtime: bool,
	window_occluded: bool,
	next_frame_deadline: Option<Instant>,
	project: Option<ProjectData>,
	input_profile_override: Option<InputProfile>,
	project_path: Option<PathBuf>,
	installed_project: bool,
	play_update_error_reported: bool,
	edit_update_error_reported: bool,
	smoke_test: bool,
	smoke_test_completed: bool,
	fatal_error: Option<String>,
}

impl GameApp {
	fn update(&mut self, delta_seconds: f32) {
		if self.standalone_runtime {
			self.update_player(delta_seconds);
			return;
		}
		if self.handle_scene_commands() {
			return;
		}
		if self.tools.panel.edit.is_some() || self.tools.drag.is_some() {
			return;
		}

		self.update_keyboard_camera(delta_seconds);

		if self.authored.is_some() {
			let previous_paused = self.editor.simulation_paused();
			let result = self.tick_play(delta_seconds);
			report_update_result(
				"development_play",
				result,
				&mut self.play_update_error_reported,
			);
			if self.editor.simulation_paused() != previous_paused {
				self.update_selection_title();
			}
		} else {
			// Structural undo can retire GPU-backed models. This is an explicit
			// key command, so the wait is never paid by ordinary movement frames.
			let control =
				self.input.is_held(Key::ControlLeft) || self.input.is_held(Key::ControlRight);
			if control
				&& (self.input.just_pressed(Key::Z) || self.input.just_pressed(Key::Y))
				&& let Err(error) = self.wait_for_gpu()
			{
				engine_logger::error!("Cannot restore scene edit: {error}");
				return;
			}
			let previous_selection = self.editor.selected_entity();
			let result = self
				.editor
				.update(&mut self.scene, &self.input, delta_seconds);
			report_update_result(
				"development_edit",
				result,
				&mut self.edit_update_error_reported,
			);
			if self.editor.selected_entity() != previous_selection {
				self.update_selection_title();
			}
		}
	}

	fn update_player(&mut self, delta_seconds: f32) {
		let completed = self.game_completed();
		// The completion message promises F10 even when gameplay normally owns it.
		if (completed && self.input.just_pressed(Key::F10))
			|| self.player_shortcut_pressed(Key::F10)
		{
			let result = self.restart_player_game();
			report_update_result(
				"player_new_game",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		if completed {
			return;
		}
		if self.player_shortcut_pressed(Key::F8) && self.editor.pending_play_scene().is_none() {
			let result = self.cycle_player_scene();
			report_update_result(
				"player_scene_switch",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		if self.player_shortcut_pressed(Key::F7)
			&& self
				.editor
				.play_progress::<progress::CampaignProgress>()
				.is_some()
		{
			let result = runtime::game_progress::ProgressAction::new::<progress::CampaignProgress>(
				progress::ProgressAction::AwardScore { amount: 1 },
			)
			.and_then(|action| self.editor.apply_play_progress(&[action]));
			report_update_result(
				"player_progress",
				result,
				&mut self.play_update_error_reported,
			);
			self.clock.reset();
			return;
		}
		self.update_keyboard_camera(delta_seconds);
		let result = self.tick_play(delta_seconds);
		report_update_result(
			"player_simulation",
			result,
			&mut self.play_update_error_reported,
		);
	}

	fn tick_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		let result = self.advance_play(delta_seconds);
		if self
			.orbit_capture
			.as_ref()
			.is_some_and(|owner| !self.editor.owns_camera_input(owner) || self.game_completed())
		{
			self.set_mouse_capture(false);
		}
		result
	}

	fn advance_play(&mut self, delta_seconds: f32) -> Result<(), String> {
		if self.input.just_pressed(Key::F8)
			&& let Some(scene_id) = self.editor.pending_play_scene().cloned()
		{
			let result = self.install_play_scene(scene_id, false);
			if result.is_err() {
				self.update_selection_title();
			}
			return result;
		}
		if self.editor.has_runtime_instances() {
			self.renderer
				.as_ref()
				.ok_or("Renderer is unavailable")?
				.wait_idle()?;
		}
		let mut outcome = self
			.editor
			.update_play(&mut self.scene, &self.input, delta_seconds)?;
		loop {
			if let Some(camera) = self.editor.play_rig_camera() {
				self.camera = camera;
			}
			if let Some(scene_id) = outcome.requested_scene {
				let result = self.install_play_scene(scene_id, false);
				if result.is_err() {
					self.update_selection_title();
				}
				return result;
			}
			self.fulfill_runtime_spawns()?;
			// synchronize_world can retire runtime models; submitted frames have
			// completed before each mutating tick above/below.
			if outcome.changes.topology_changed
				&& let Some(assets) = &mut self.asset_manager
			{
				assets.prune_unused();
			}
			if outcome.pending_steps == 0 {
				break;
			}
			if self.editor.has_runtime_instances() {
				self.renderer
					.as_ref()
					.ok_or("Renderer is unavailable")?
					.wait_idle()?;
			}
			outcome = self.editor.drain_play(&mut self.scene)?;
		}
		Ok(())
	}

	fn fulfill_runtime_spawns(&mut self) -> Result<(), String> {
		if self.editor.simulation_paused() {
			return Ok(());
		}
		for request in self.editor.pending_runtime_spawns() {
			let result = (|| {
				let root = self
					.asset_root
					.as_ref()
					.ok_or("Asset root is unavailable")?;
				let (prefab, fingerprint) =
					bridge::runtime_prefab::load(root, &request.prefab_path)?;
				let mut prepared = self.editor.prepare_runtime_spawn(
					&prefab,
					&fingerprint,
					request.position,
					Some(request.id),
				)?;
				let data = self.editor.runtime_spawn_document(&prepared)?;
				let renderer = self.renderer.as_ref().ok_or("Renderer is unavailable")?;
				let assets = self
					.asset_manager
					.as_mut()
					.ok_or("Assets are unavailable")?;
				let candidate = scene_io::instantiate(&data, assets, renderer);
				renderer.wait_idle()?;
				let (scene, _) = candidate?;
				let keys: std::collections::HashSet<_> = prepared
					.entities()
					.iter()
					.map(|entity| entity.key.clone())
					.collect();
				prepared.set_visibility_capabilities(
					scene
						.visibility_capabilities()
						.into_iter()
						.filter(|key| keys.contains(key))
						.collect(),
				)?;
				self.editor.commit_runtime_spawn(prepared)?;
				let mut scene = scene;
				if let Some(bank) = self.editor.animations() {
					scene.synchronize_animation(bank)?;
				}
				self.scene = scene;
				Ok::<(), String>(())
			})();
			if let Err(error) = result {
				self.editor.reject_runtime_spawn(request.id, &error)?;
				engine_logger::error!(target:"game", "Runtime spawn rejected: operation=prepare_and_admit_runtime_spawn");
			}
			if let Some(assets) = &mut self.asset_manager {
				assets.prune_unused();
			}
		}
		Ok(())
	}

	fn player_shortcut_pressed(&self, key: Key) -> bool {
		self.input.just_pressed(key) && !self.editor.play_input_binds_key(key)
	}

	fn game_completed(&self) -> bool {
		self.editor
			.play_status()
			.and_then(|status| status.gameplay)
			.is_some_and(|status| status.completed)
	}

	fn update_keyboard_camera(&mut self, delta_seconds: f32) {
		if self.editor.play_rig_camera().is_some() {
			return;
		}
		let game_owns_arrow = self.editor.is_playing()
			&& [
				Key::ArrowLeft,
				Key::ArrowRight,
				Key::ArrowDown,
				Key::ArrowUp,
			]
			.into_iter()
			.any(|key| self.input.is_held(key) && self.editor.play_input_binds_key(key));
		if !game_owns_arrow {
			self.camera_controller
				.update(&mut self.camera, &self.input, delta_seconds);
		}
	}

	fn needs_continuous_redraw(&self) -> bool {
		if self.window_occluded || (self.standalone_runtime && self.game_completed()) {
			return false;
		}

		if self
			.window
			.as_ref()
			.is_some_and(|window| !window.has_focus())
		{
			return false;
		}

		self.editor.needs_tick()
			|| self.editor.play_movement_held(&self.input)
			|| (!self.standalone_runtime
				&& self.authored.is_none()
				&& MovementBindings::WASD.any_held(&self.input))
			|| (MovementBindings::ARROWS.any_held(&self.input)
			&& self.editor.play_rig_camera().is_none())
	}

	fn request_input_frame(&mut self) {
		if !self.needs_continuous_redraw() {
			self.clock.reset();

			self.next_frame_deadline = None;
		}
		if let Some(window) = self.window.as_ref() {
			window.request_redraw();
		}
	}

	fn handle_scene_commands(&mut self) -> bool {
		// The first pressed command wins. Reload precedes save to protect disk edits.
		let command = [
			(Key::F9, SceneCommand::Reload),
			(Key::F5, SceneCommand::Save),
			(Key::F6, SceneCommand::TogglePlay),
			(Key::Delete, SceneCommand::DeleteSelected),
			(Key::C, SceneCommand::DuplicateSelected),
			(Key::N, SceneCommand::SpawnRobot),
		]
		.into_iter()
		.find_map(|(key, command)| self.input.just_pressed(key).then_some(command));
		let Some(command) = command else {
			return false;
		};

		let (action, result) = self.execute_scene_command(command);
		if let Err(error) = result {
			engine_logger::error!("Failed to {action}: {error}");
		}

		if matches!(
			command,
			SceneCommand::DeleteSelected
				| SceneCommand::DuplicateSelected
				| SceneCommand::SpawnRobot
		) {
			self.update_selection_title();
		}
		self.clock.reset();

		true
	}

	fn execute_scene_command(
		&mut self,
		command: SceneCommand,
	) -> (&'static str, Result<(), String>) {
		match command {
			SceneCommand::Reload => ("reload scene", self.reload_scene()),
			SceneCommand::Save => ("save scene", self.save_scene()),
			SceneCommand::TogglePlay => ("change Edit/Play mode", self.toggle_play()),
			SceneCommand::DeleteSelected => {
				("delete selected entity", self.apply_entity_command(command))
			}
			SceneCommand::DuplicateSelected => (
				"duplicate selected entity",
				self.apply_entity_command(command),
			),
			SceneCommand::SpawnRobot => ("spawn robot", self.apply_entity_command(command)),
		}
	}

	fn update_selection_title(&mut self) {
		let scene_status = self
			.editor
			.play_scene_id()
			.map(|id| format!(" | Scene: {}", id.as_str()))
			.unwrap_or_default();
		let pending = self
			.editor
			.pending_play_scene()
			.map(|id| format!(" | {} pending (F8 to retry)", id.as_str()))
			.unwrap_or_default();
		if self.standalone_runtime {
			let mut title = self
				.project_name
				.as_deref()
				.unwrap_or("Game Engine Player")
				.to_owned();
			if let Some(gameplay) = self.editor.play_status().and_then(|status| status.gameplay) {
				title.push_str(&format!(" | {}", gameplay.objective));
				if let Some(detail) = gameplay.detail {
					title.push_str(&format!(" | {detail}"));
				}
				if let Some(score) = gameplay.score {
					title.push_str(&format!(" | Score: {score}"));
				}
				if gameplay.completed {
					title.push_str(" | Complete! F10 to restart");
				}
			}
			if self.editor.simulation_paused() {
				title.push_str(" | Paused");
			}
			title.push_str(&scene_status);
			title.push_str(&pending);
			if title != self.last_window_title {
				if let Some(window) = self.window.as_ref() {
					window.set_title(&title);
				}
				self.last_window_title = title;
			}
			return;
		}
		let Some(window) = self.window.as_ref() else {
			return;
		};
		let selected_name = self
			.editor
			.selected_entity()
			.and_then(|id| self.scene.name(id))
			.unwrap_or("None");

		let mode = if self.authored.is_none() {
			"Edit"
		} else if self.editor.simulation_paused() {
			"Play paused"
		} else {
			"Play"
		};

		let title = format!(
			"Game Engine{} | {mode} | Selected: {selected_name} | F6 Edit/Play{scene_status}{pending}",
			if self.is_dirty() { " *" } else { "" }
		);
		if title != self.last_window_title {
			window.set_title(&title);
			self.last_window_title = title;
		}
	}

	fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let window = Arc::new(
			event_loop
				.create_window(
					Window::default_attributes()
						.with_visible(!self.smoke_test)
						.with_title(self.project_name.as_deref().unwrap_or("Game Engine"))
						.with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
				)
				.map_err(|e| e.to_string())?,
		);

		let size = window.inner_size();
		let renderer = WindowRenderer::new(Arc::clone(&window), size.width, size.height)?;
		let asset_root = if self.installed_project {
			std::env::current_exe()
				.map_err(|e| e.to_string())?
				.parent()
				.ok_or("executable has no parent")?
				.join("assets")
		} else {
			PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
		};

		let startup_scene = match self.project.as_ref() {
			Some(project) => project.startup_scene_path()?,
			None => "scenes/robots.json",
		};
		let scene_path = asset_root.join(startup_scene);
		let mut loaded = scene_io::load(&scene_path, &asset_root, &renderer)?;
		let mut editor = Self::prepare_editor(&mut loaded)?;
		let project = match self.project.clone() {
			Some(project) => project,
			None if self.installed_project => {
				let mut project = game::legacy_project(
					&loaded.scene,
					self.project_name.as_deref().unwrap_or("Legacy Project"),
					startup_scene,
				)?;
				project.gameplay_module = Some(progress::COURTYARD_RUN_ID.to_owned());
				project
			}
			None => ProjectData::for_scene(
				self.project_name.as_deref().unwrap_or("Game Engine"),
				startup_scene,
				&loaded.scene.to_data(&loaded.camera)?,
			)?,
		};
		editor.set_initial_project(project.clone())?;
		editor.set_module_resolver(Arc::new(progress::resolve))?;
		let overlay = if !self.standalone_runtime {
			Some(SelectionOverlay::new(&renderer, &mut loaded.assets)?)
		} else {
			None
		};
		let mut tools = EditorTools {
			saved: if self.standalone_runtime {
				None
			} else {
				Some(loaded.scene.to_data(&loaded.camera)?)
			},
			..EditorTools::default()
		};
		if !self.standalone_runtime {
			tools.panel.initialize(&renderer, &mut loaded.assets)?;
			tools.gizmo = Some(gizmo_overlay::GizmoOverlay::new(
				&renderer,
				&mut loaded.assets,
			)?);
			tools.models = runtime::assets::catalog::scan_models(&asset_root)?;
		}

		// Install only after every fallible initialization step has succeeded.
		if self.standalone_runtime {
			editor.start_play(&mut loaded.scene)?;
		}
		self.scene = loaded.scene;
		self.asset_manager = Some(loaded.assets);
		self.camera = loaded.camera;
		if let Some(profile) = &self.input_profile_override {
			editor.set_play_input_profile(profile.clone(), &mut self.input);
		}
		self.editor = editor;
		self.tools = tools;
		self.selection_overlay = overlay;
		self.project = Some(project);
		self.asset_root = Some(asset_root);
		self.scene_path = Some(scene_path);
		self.renderer = Some(renderer);
		self.window = Some(window);
		self.clock.reset();
		self.update_selection_title();
		engine_logger::component_health(
			"renderer",
			"HEALTHY",
			false,
			"Game rendering resources initialized",
		);
		Ok(())
	}

	fn save_scene(&mut self) -> Result<(), String> {
		let path = self
			.scene_path
			.as_deref()
			.ok_or("Scene path is not initialized")?;
		let (scene, camera) = match self.authored.as_ref() {
			Some(authored) => (&authored.scene, &authored.camera),
			None => (&self.scene, &self.camera),
		};
		let data = scene.to_data(camera)?;
		let project = self
			.editor
			.project()
			.cloned()
			.ok_or("Project configuration is not initialized")?;
		project.validate_scene(&data)?;
		let project_path = self
			.project_path
			.clone()
			.or_else(|| {
				self.asset_root
					.as_ref()
					.and_then(|root| root.parent())
					.map(|root| root.join("project.json"))
			})
			.ok_or("Project path is not initialized")?;
		scene_io::save_data(path, &data)?;
		scene_io::save_project_data(&project_path, &project).map_err(|error| {
			format!(
				"Scene was saved, but project settings were not saved: {error}. The document remains dirty."
			)
		})?;
		self.project_path = Some(project_path);
		self.project = Some(project);
		self.tools.saved = Some(data);
		self.editor.mark_saved()?;
		engine_logger::info!("Saved scene: {}", path.display());
		self.update_selection_title();
		Ok(())
	}

	fn wait_for_gpu(&self) -> Result<(), String> {
		if let Some(renderer) = self.renderer.as_ref() {
			renderer.wait_idle()?;
		}
		Ok(())
	}

	fn prepare_editor(loaded: &mut scene_io::LoadedScene) -> Result<SceneEditor, String> {
		if loaded.source_schema_version == 1
			&& let [left_robot, right_robot, ..] = loaded.entity_ids.as_slice()
		{
			loaded.scene.set_binding("robot_a", *left_robot)?;
			loaded.scene.set_binding("robot_b", *right_robot)?;
		}
		let data = loaded.scene.to_data(&loaded.camera)?;
		let project = ProjectData::for_scene("Game Engine", "scenes/robots.json", &data)?;
		SceneEditor::for_project(&loaded.scene, &loaded.camera, project)
	}

	fn apply_entity_command(&mut self, command: SceneCommand) -> Result<(), String> {
		if self.authored.is_some() {
			return Err("Stop Play mode with F6 before editing entities".to_string());
		}
		self.wait_for_gpu()?;
		match command {
			SceneCommand::DeleteSelected => self.editor.delete_selected(&mut self.scene),
			SceneCommand::DuplicateSelected => {
				self.editor.duplicate_selected(&mut self.scene).map(|_| ())
			}
			SceneCommand::SpawnRobot => self.spawn_robot(),
			_ => Err("Expected an entity editing command".to_string()),
		}
	}

	fn spawn_robot(&mut self) -> Result<(), String> {
		if self.spawn_template.is_none() {
			let existing = self
				.scene
				.entity_ids()
				.find(|id| self.scene.model_path(*id) == Some("props/robot.glb"))
				.and_then(|id| self.scene.get(id))
				.cloned();
			let mut instance = match existing {
				Some(instance) => instance,
				None => {
					let renderer = self
						.renderer
						.as_ref()
						.ok_or("Renderer is not initialized")?;
					let assets = self
						.asset_manager
						.as_mut()
						.ok_or("Assets are not initialized")?;
					let model = assets
						.load_model(renderer, "props/robot.glb")
						.map_err(|error| error.to_string())?;
					ModelInstance::new(Arc::new(model), Transform::default())
				}
			};
			*instance.transform_mut() = Transform {
				scale: [0.7; 3],
				..Transform::default()
			};
			self.spawn_template = Some(instance);
		}
		self.editor.spawn(
			&mut self.scene,
			"Robot",
			"props/robot.glb",
			self.spawn_template
				.as_ref()
				.ok_or("Robot template is missing")?
				.clone(),
		)?;
		Ok(())
	}

	fn toggle_play(&mut self) -> Result<(), String> {
		if self.authored.is_some() {
			self.stop_play()
		} else {
			self.start_play()
		}
	}

	fn start_play(&mut self) -> Result<(), String> {
		if self.authored.is_some() {
			return Ok(());
		}
		self.cancel_drag();
		let mut play_scene = self.scene.clone();
		self.editor.start_play(&mut play_scene)?;
		let authored_scene = std::mem::replace(&mut self.scene, play_scene);
		self.authored = Some(AuthoredScene {
			scene: authored_scene,
			camera: self.camera.clone(),
		});
		self.finish_mode_change();
		Ok(())
	}

	fn stop_play(&mut self) -> Result<(), String> {
		self.wait_for_gpu()?;
		let Some(authored) = self.authored.as_mut() else {
			return Ok(());
		};
		self.editor.stop_play(&mut authored.scene)?;
		let authored = self.authored.take().expect("authored scene was present");
		self.scene = authored.scene;
		self.camera = authored.camera;
		self.finish_mode_change();
		Ok(())
	}

	fn finish_mode_change(&mut self) {
		self.tools.panel.edit = None;
		self.set_mouse_capture(false);
		self.input.suppress_held();
		self.editor.end_gesture();
		self.clock.reset();
		self.update_selection_title();
	}

	fn cycle_player_scene(&mut self) -> Result<(), String> {
		let current = self
			.editor
			.play_scene_id()
			.cloned()
			.ok_or("Play mode is not active")?;
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?;
		let mut scene_ids: Vec<_> = project
			.scenes
			.iter()
			.map(|scene| scene.id().clone())
			.collect();
		scene_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
		if scene_ids.len() < 2 {
			return Ok(());
		}
		let current_index = scene_ids
			.iter()
			.position(|id| id == &current)
			.ok_or("Running scene is absent from the project catalog")?;
		let next = scene_ids[(current_index + 1) % scene_ids.len()].clone();
		self.switch_player_scene(next)
	}

	fn switch_player_scene(&mut self, scene_id: SceneId) -> Result<(), String> {
		self.install_play_scene(scene_id, false)
	}

	fn restart_player_game(&mut self) -> Result<(), String> {
		let startup = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?
			.startup_scene_id
			.clone();
		self.install_play_scene(startup, true)
	}

	fn install_play_scene(&mut self, scene_id: SceneId, new_game: bool) -> Result<(), String> {
		let current = self
			.editor
			.play_scene_id()
			.ok_or("Play mode is not active")?;
		if !new_game && current == &scene_id {
			return Ok(());
		}
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is not initialized")?;
		let relative_path = project.scene_path(&scene_id)?;
		let asset_root = self
			.asset_root
			.as_ref()
			.ok_or("Asset root is not initialized")?
			.canonicalize()
			.map_err(|error| error.to_string())?;
		let scene_path = asset_root
			.join(relative_path)
			.canonicalize()
			.map_err(|error| error.to_string())?;
		if !scene_path.starts_with(&asset_root) {
			return Err("Scene path is outside the project asset root".into());
		}
		let parsed = scene_io::load_data(&scene_path)?;
		let document = Arc::new(parsed.data);
		let prepared = if new_game {
			self.editor
				.prepare_new_game(scene_id, Arc::clone(&document))?
		} else {
			self.editor
				.prepare_play_scene(scene_id, Arc::clone(&document))?
		};
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;
		let assets = self
			.asset_manager
			.as_mut()
			.ok_or("Assets are not initialized")?;
		let candidate = scene_io::instantiate(&document, assets, renderer);

		// Both scenes share one cache and budget until the candidate commits.
		// Completion also makes cache-only allocations from rejected loads safe to prune.
		renderer.wait_idle()?;
		let installed = candidate.and_then(|(scene, camera)| {
			self.editor
				.commit_play_scene(prepared, &mut self.input, self.authored.is_some())?;
			// No fallible operations between committing the CPU world and its view.
			self.scene = scene;
			self.camera = self.editor.play_rig_camera().unwrap_or(camera);
			Ok(())
		});
		assets.prune_unused();
		installed?;
		self.set_mouse_capture(false);
		if self.standalone_runtime {
			self.scene_path = Some(scene_path);
		}
		self.clock.reset();
		self.next_frame_deadline = None;
		self.play_update_error_reported = false;
		self.update_selection_title();
		if let Some(window) = self.window.as_ref() {
			window.request_redraw();
		}
		Ok(())
	}

	fn reload_scene(&mut self) -> Result<(), String> {
		let old_path = self
			.scene_path
			.clone()
			.ok_or("Scene path is not initialized")?;
		let asset_root = self
			.asset_root
			.as_deref()
			.ok_or("Asset root is not initialized")?;
		let project = self
			.project_path
			.as_ref()
			.map(ProjectData::load)
			.transpose()?
			.or_else(|| self.project.clone());
		let path = match project.as_ref() {
			Some(project) => asset_root.join(project.startup_scene_path()?),
			None => old_path,
		};
		let renderer = self
			.renderer
			.as_ref()
			.ok_or("Renderer is not initialized")?;

		let mut loaded = scene_io::load(&path, asset_root, renderer)?;
		let mut editor = Self::prepare_editor(&mut loaded)?;
		if let Some(project) = project.as_ref() {
			editor.set_initial_project(project.clone())?;
		}
		editor.set_module_resolver(Arc::new(progress::resolve))?;
		if self.standalone_runtime {
			editor.start_play(&mut loaded.scene)?;
		}
		let saved = loaded.scene.to_data(&loaded.camera)?;

		// Drawing can still reference the old meshes and textures. Finish all
		// GPU work before assignments release the old scene, then its cache.
		renderer.wait_idle()?;
		self.scene = loaded.scene;
		self.authored = None;
		self.spawn_template = None;
		self.tools.templates.clear();
		self.tools.drag = None;
		self.tools.saved = Some(saved);
		if let Some(profile) = &self.input_profile_override {
			editor.set_play_input_profile(profile.clone(), &mut self.input);
		}
		self.editor = editor;
		self.asset_manager = Some(loaded.assets);
		self.camera = loaded.camera;
		self.project = project;
		self.scene_path = Some(path.clone());
		self.finish_mode_change();
		engine_logger::info!("Reloaded scene: {}", path.display());
		Ok(())
	}

	fn render(&mut self) -> Result<renderer::DrawStatus, String> {
		let hud_rows = if self.standalone_runtime {
			gameplay_hud_rows(self.editor.play_status().as_ref())
		} else {
			Vec::new()
		};
		if let Some(camera) = self.editor.play_rig_camera() {
			self.camera = camera;
		}
		let dirty = !self.standalone_runtime && self.is_dirty();
		let Some(renderer) = self.renderer.as_mut() else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let Some([width, height]) = renderer.prepare()? else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let camera_uniform = CameraUniform::perspective(
			self.camera.position,
			self.camera.target(),
			[0.0, 1.0, 0.0],
			self.camera.vertical_fov,
			width as f32 / height as f32,
			self.camera.near,
			self.camera.far,
		);

		let frustum = if self.tools.culling {
			Some(runtime::visibility::Frustum::from_view_projection(
				glam::Mat4::from_cols_array(&camera_uniform.view_projection),
			)?)
		} else {
			None
		};
		let (mut objects, mut stats) = self.scene.try_render_visible(frustum.as_ref())?;
		if !self.standalone_runtime
			&& self.authored.is_none()
			&& let Some(object) = self
				.selection_overlay
				.as_ref()
				.and_then(|overlay| overlay.object(&self.scene, &self.editor))
		{
			stats.draw_calls += 1;
			stats.triangles += object.mesh().index_count() as usize / 3;
			objects.push(object);
		}

		if !self.standalone_runtime {
			if self.authored.is_none()
				&& let Some(transform) = self
					.editor
					.selected_entity()
					.map(|id| runtime::gizmo::GizmoPose::from_scene(&self.scene, id))
					.transpose()?
				&& let Some(gizmo) = self.tools.gizmo.as_mut()
			{
				gizmo.prepare(
					renderer,
					&self.camera,
					[width, height],
					transform,
					self.tools.panel.mode,
					self.tools.drag.as_ref().map(|drag| drag.axis),
				)?;
				let object = gizmo.object();
				stats.draw_calls += 1;
				stats.triangles += object.mesh().index_count() as usize / 3;
				objects.push(object);
			}
			let scale = self
				.window
				.as_ref()
				.map_or(1.0, |window| window.scale_factor());
			// The panel is one batched draw. Include its previous geometry in
			// displayed totals; the current text has the same quad count.
			if let Some(panel) = self.tools.panel.object() {
				stats.draw_calls += 1;
				stats.triangles += panel.mesh().index_count() as usize / 3;
			}
			self.tools.panel.prepare(
				renderer,
				[width, height],
				scale,
				PanelView {
					scene: &self.scene,
					editor: &self.editor,
					models: &self.tools.models,
					playing: self.authored.is_some(),
					dirty,
					stats,
					frame_ms: self.tools.metrics.frame_ms,
					cpu_ms: self.tools.metrics.cpu_ms,
					culling: self.tools.culling,
				},
			)?;
			if let Some(panel) = self.tools.panel.object() {
				objects.push(panel);
			}
		}
		if self.standalone_runtime && !hud_rows.is_empty() {
			if self.game_hud.is_none() {
				self.game_hud = Some(renderer::ui::ScreenOverlay::new(renderer)?);
			}
			let hud = self.game_hud.as_mut().expect("initialized HUD");
			let scale = self
				.window
				.as_ref()
				.map_or(1.0, |window| window.scale_factor());
			hud.prepare(renderer, [width, height], scale, &hud_rows)?;
			if let Some(object) = hud.object() {
				objects.push(object);
			}
		}
		self.tools.stats = stats;

		renderer.set_lights(&self.scene.renderer_lights())?;
		renderer.draw(&camera_uniform, &objects)
	}

	fn pick_entity(&mut self) -> Result<(), String> {
		if self.standalone_runtime || self.authored.is_some() || self.mouse_captured {
			return Ok(());
		}
		let Some(pixel) = self.cursor_position else {
			return Ok(());
		};
		let window = self.window.as_ref().ok_or("Window is not initialized")?;
		let size = window.inner_size();
		if size.width == 0 || size.height == 0 {
			return Ok(());
		}
		let Some(extent) = self
			.renderer
			.as_mut()
			.ok_or("Renderer is not initialized")?
			.prepare()?
		else {
			return Ok(());
		};
		let pixel = [
			pixel[0] * extent[0] as f64 / size.width as f64,
			pixel[1] * extent[1] as f64 / size.height as f64,
		];
		self.editor.pick(&self.scene, &self.camera, pixel, extent)?;
		self.update_selection_title();
		Ok(())
	}

	fn submit_captured_orbit(&mut self, intent: Result<CameraIntent, String>) {
		let Some(owner) = &self.orbit_capture else {
			return;
		};
		let Ok(intent) = intent else { return };
		match self.editor.submit_camera_input(owner, intent) {
			Ok(()) => {
				if let Some(window) = &self.window {
					window.request_redraw();
				}
			}
			Err(error) => {
				engine_logger::debug!("Play camera input rejected: {error}");
				self.set_mouse_capture(false);
			}
		}
	}

	fn captured_motion(&mut self, dx: f64, dy: f64) {
		if self.orbit_capture.is_some() {
			self.submit_captured_orbit(normalize_mouse(dx, dy, Default::default()));
		} else if self.editor.play_rig_camera().is_none() {
			self.request_input_frame();
			self.camera_controller
				.mouse_motion(&mut self.camera, dx, dy);
		}
	}

	fn set_mouse_capture(&mut self, capture: bool) {
		if !capture && let Some(owner) = self.orbit_capture.take() {
			self.input.suppress_held();
			if self.editor.owns_camera_input(&owner) {
				self.editor.clear_play_input();
			}
		}
		let Some(window) = self.window.as_ref() else {
			self.mouse_captured = false;
			return;
		};

		if capture {
			let size = window.inner_size();
			if self.mouse_captured
				|| !window.has_focus()
				|| self.window_occluded
				|| size.width == 0
				|| size.height == 0
			{
				return;
			}

			let orbit_owner = self.editor.camera_input_owner();
			if self.editor.play_rig_camera().is_some()
				&& (orbit_owner.is_none() || self.game_completed())
			{
				return;
			}
			let result = window
				.set_cursor_grab(CursorGrabMode::Locked)
				.or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));

			match result {
				Ok(()) => {
					window.set_cursor_visible(false);
					self.orbit_capture = orbit_owner;
					self.mouse_captured = true;
				}
				Err(error) => {
					engine_logger::error!("Failed to capture mouse: {error}");
				}
			}
		} else {
			// Always stop mouse look and restore visibility, even if releasing fails.
			self.mouse_captured = false;
			if let Err(error) = window.set_cursor_grab(CursorGrabMode::None) {
				engine_logger::error!("Failed to release mouse: {error}");
			}
			window.set_cursor_visible(true);
		}
	}
}

impl ApplicationHandler for GameApp {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}
		if let Err(error) = self
			.initialize(event_loop)
			.map_err(std::io::Error::other)
			.log_error_for("game::renderer", "initialize game")
		{
			self.fatal_error = Some(error.to_string());
			event_loop.exit();
			return;
		}
		if self.smoke_test {
			// Hidden windows may not receive redraw events. Exercise the normal
			// rendering path here on the executable's ordinary main-thread stack.
			// A bounded retry tolerates an initial swapchain recreation.
			let result = (|| {
				for _ in 0..8 {
					if self.render()? == renderer::DrawStatus::Rendered {
						self.wait_for_gpu()?;
						return Ok(());
					}
				}
				Err("Startup smoke test did not render a frame".to_owned())
			})();
			match result {
				Ok(()) => {
					self.smoke_test_completed = true;
					println!(
						"GAME_STARTUP_SMOKE_OK mode={}",
						if self.standalone_runtime {
							"player"
						} else {
							"development"
						}
					);
				}
				Err(error) => self.fatal_error = Some(error),
			}
			event_loop.exit();
			return;
		}
		self.window.as_ref().unwrap().request_redraw();
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		window_id: WindowId,
		event: WindowEvent,
	) {
		if self
			.window
			.as_ref()
			.is_none_or(|window| window.id() != window_id)
		{
			return;
		}

		match event {
			WindowEvent::CloseRequested => event_loop.exit(),
			WindowEvent::CursorMoved { position, .. } => {
				self.cursor_position = Some([position.x, position.y]);
				if !self.standalone_runtime
					&& let Err(error) = self.preview_drag()
				{
					self.report_editor_error(error);
				}
				if self.tools.drag.is_some() {
					self.request_input_frame();
				}
			}
			WindowEvent::CursorLeft { .. } => {
				self.cursor_position = None;
			}
			WindowEvent::MouseInput { state, button, .. } => {
				// Record first so editor mode/property changes can suppress the click.
				let input_button = native_input::mouse_button(button);
				match state {
					ElementState::Pressed => self.input.press(input_button),
					ElementState::Released => self.input.release(input_button),
				}
				match (button, state) {
					(MouseButton::Left, ElementState::Pressed) if !self.standalone_runtime => {
						if let Err(error) = self.left_press() {
							self.report_editor_error(error);
						}
					}
					(MouseButton::Left, ElementState::Released) if !self.standalone_runtime => {
						if let Err(error) = self.finish_drag() {
							self.report_editor_error(error);
						}
					}
					(MouseButton::Right, ElementState::Pressed) => {
						if !self.standalone_runtime {
							self.cancel_drag();
							self.tools.panel.edit = None;
						}
						self.set_mouse_capture(true);
					}
					_ => {}
				}
				self.request_input_frame();
			}
			WindowEvent::MouseWheel { delta, .. } if self.orbit_capture.is_some() => {
				if self
					.window
					.as_ref()
					.is_some_and(|window| window.has_focus())
				{
					let sample = match delta {
						MouseScrollDelta::LineDelta(_, y) => WheelSample::Lines(f64::from(y)),
						MouseScrollDelta::PixelDelta(position) => WheelSample::Pixels(position.y),
					};
					self.submit_captured_orbit(normalize_wheel(sample));
				}
			}
			WindowEvent::MouseWheel { delta, .. } if !self.standalone_runtime => {
				if let Some(pixel) = self.cursor_position {
					let scale = self
						.window
						.as_ref()
						.map_or(1.0, |window| window.scale_factor());
					let lines = match delta {
						MouseScrollDelta::LineDelta(_, y) => y,
						MouseScrollDelta::PixelDelta(position) => position.y as f32 / 26.0,
					};
					self.tools.panel.scroll(pixel, scale, lines);
					self.request_input_frame();
				}
			}
			WindowEvent::KeyboardInput {
				event,
				is_synthetic,
				..
			} => {
				// Winit synthesizes presses for keys already held when focus returns.
				// Those are not new gameplay presses; releases still re-arm suppression.
				if is_synthetic && event.state == ElementState::Pressed {
					return;
				}
				if event.state == ElementState::Pressed
					&& self.mouse_captured
					&& event.physical_key == PhysicalKey::Code(winit::keyboard::KeyCode::Escape)
				{
					self.set_mouse_capture(false);
					self.request_input_frame();
					return;
				}
				if !self.standalone_runtime
					&& event.state == ElementState::Pressed
					&& let PhysicalKey::Code(code) = event.physical_key
					&& self.editor_key(code, &event)
				{
					self.request_input_frame();
					return;
				}

				let Some(key) = native_input::keyboard_key(event.physical_key) else {
					return;
				};

				self.request_input_frame();

				match event.state {
					ElementState::Pressed if !event.repeat => self.input.press(key),
					ElementState::Pressed => {}
					ElementState::Released => self.input.release(key),
				}
				if let Err(error) = self.editor.sample_play_keyboard(&self.input) {
					engine_logger::error!(target: "game", "Gameplay input rejected: {error}");
				}
			}
			WindowEvent::Focused(false) => {
				if !self.standalone_runtime {
					self.cancel_drag();
					self.tools.panel.edit = None;
				}
				self.set_mouse_capture(false);
				self.input.suppress_held();
				self.editor.clear_play_input();
				self.clock.reset();

				self.next_frame_deadline = None;
			}
			WindowEvent::Focused(true) => {
				self.clock.reset();

				self.next_frame_deadline = None;
				self.window.as_ref().unwrap().request_redraw();
			}
			WindowEvent::Occluded(occluded) => {
				self.window_occluded = occluded;
				self.clock.reset();

				self.next_frame_deadline = None;
				if occluded {
					self.set_mouse_capture(false);
					self.input.suppress_held();
					self.editor.clear_play_input();
				} else {
					self.window.as_ref().unwrap().request_redraw();
				}
			}
			WindowEvent::Resized(size) => {
				if !self.standalone_runtime {
					self.cancel_drag();
				}
				if let Some(renderer) = self.renderer.as_mut() {
					renderer.resize(size.width, size.height);
				}
				if size.width == 0 || size.height == 0 {
					if !self.standalone_runtime {
						self.cancel_drag();
					}
					self.set_mouse_capture(false);
					self.input.suppress_held();
					self.editor.clear_play_input();
					self.clock.reset();

					self.next_frame_deadline = None;
				} else {
					self.window.as_ref().unwrap().request_redraw();
				}
			}
			WindowEvent::RedrawRequested => {
				if self
					.last_heartbeat
					.is_none_or(|last| last.elapsed().as_secs() >= 5)
				{
					engine_logger::heartbeat("game");
					self.last_heartbeat = Some(Instant::now());
				}
				let size = self.window.as_ref().unwrap().inner_size();

				if size.width == 0 || size.height == 0 || self.window_occluded {
					self.clock.reset();

					self.next_frame_deadline = None;
					return;
				}

				let delta_seconds =
					if self.standalone_runtime && !self.window.as_ref().unwrap().has_focus() {
						self.clock.reset();

						0.0
					} else {
						self.clock.tick(Instant::now()).as_secs_f32()
					};
				let update_start = Instant::now();
				self.update(delta_seconds);
				self.tools
					.metrics
					.record(update_start, update_start.elapsed().as_secs_f32());
				self.input.end_frame();
				self.update_selection_title();

				if let Err(error) = self
					.render()
					.map_err(std::io::Error::other)
					.log_error_for("game::renderer", "render frame")
				{
					self.fatal_error = Some(error.to_string());
					event_loop.exit();
					return;
				}
				if self.needs_continuous_redraw() {
					// Measure from update start, so time already spent in
					// FIFO presentation counts toward this frame's budget.
					// Starting a fresh delay after draw would halve a
					// 60 Hz display to roughly 30 frames per second.
					self.next_frame_deadline =
						Some(update_start + Duration::from_nanos(16_666_667));
				} else {
					self.clock.reset();

					self.next_frame_deadline = None;
				}
			}
			_ => {}
		}
	}

	fn device_event(
		&mut self,
		_event_loop: &ActiveEventLoop,
		_device_id: DeviceId,
		event: DeviceEvent,
	) {
		if !self.mouse_captured
			|| self
				.window
				.as_ref()
				.is_none_or(|window| !window.has_focus())
		{
			return;
		}

		if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
			self.captured_motion(dx, dy);
		}
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		let Some(window) = self.window.as_ref() else {
			event_loop.set_control_flow(ControlFlow::Wait);
			return;
		};
		let size = window.inner_size();
		if !self.needs_continuous_redraw() || size.width == 0 || size.height == 0 {
			self.next_frame_deadline = None;
			event_loop.set_control_flow(ControlFlow::Wait);
			return;
		}
		match self.next_frame_deadline {
			Some(deadline) if deadline > Instant::now() => {
				event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
			}
			_ => {
				// Input and resize handlers can still request an immediate
				// frame. Only continuously scheduled redraws use this cap.
				window.request_redraw();
				event_loop.set_control_flow(ControlFlow::Wait);
			}
		}
	}

	fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
		self.set_mouse_capture(false);
	}
}

impl Drop for GameApp {
	fn drop(&mut self) {
		// Transfer every external GPU owner so an uncertain wait retains them together.
		if let Some(renderer) = self.renderer.as_ref() {
			let _ = renderer
				.retire_resources((
					std::mem::take(&mut self.scene),
					self.authored.take(),
					std::mem::take(&mut self.editor),
					std::mem::take(&mut self.tools),
					self.selection_overlay.take(),
					self.game_hud.take(),
					self.spawn_template.take(),
					self.asset_manager.take(),
				))
				.map_err(std::io::Error::other)
				.log_error_for("game::renderer", "wait before releasing scene");
		}
	}
}

fn gameplay_hud_rows(status: Option<&runtime::editor::command::PlayStatusDto>) -> Vec<String> {
	let Some(status) = status else {
		return Vec::new();
	};
	let mut rows = Vec::new();
	if let Some(gameplay) = &status.gameplay {
		rows.push(gameplay.objective.clone());
		if let Some(detail) = &gameplay.detail {
			rows.push(detail.clone());
		}
		if let Some(score) = gameplay.score {
			rows.push(format!("Score {score}"));
		}
		if gameplay.completed {
			rows.push("F10 to restart".into());
		}
	}
	if status.paused {
		rows.push("Paused".into());
	}
	if let Some(scene) = &status.pending_scene_id {
		rows.push(format!("Scene {} pending. F8 to retry", scene.as_str()));
	}
	rows
}

fn main() -> Result<(), String> {
	let _logging =
		engine_logger::init_application("game").expect("failed to initialize game logger");

	run().inspect_err(|_| {
		engine_logger::error!(target: "game", "Game launch failed: operation=startup");
	})
}

fn run() -> Result<(), String> {
	let mut app = GameApp::default();

	let executable = std::env::current_exe().map_err(|error| error.to_string())?;
	let executable_root = executable
		.parent()
		.ok_or("Executable has no parent directory")?;
	let installed_config = executable_root.join("project.json");
	let marker = executable_root.join("project-name.txt");
	let has_manifest = installed_config.is_file();
	let legacy_install = !has_manifest && marker.is_file();
	app.installed_project = has_manifest || legacy_install;
	if legacy_install {
		app.project_name = read_legacy_project_name(&marker);
	}
	let source_config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.json");
	app.project_path = if has_manifest {
		Some(installed_config)
	} else if !app.installed_project && source_config.is_file() {
		Some(source_config)
	} else {
		None
	};
	app.project = app
		.project_path
		.as_ref()
		.map(ProjectData::load)
		.transpose()?;
	if let Some(project) = app.project.as_ref() {
		app.project_name = Some(project.name.clone());
	}
	let arguments = std::env::args_os()
		.skip(1)
		.map(|argument| {
			argument
				.into_string()
				.map_err(|_| "Game arguments must contain valid Unicode".to_owned())
		})
		.collect::<Result<Vec<_>, _>>()?;
	let options = LaunchOptions::parse(
		arguments,
		app.installed_project,
		std::env::var("GAME_ENGINE_NATIVE_EDITOR_DEMO").as_deref() == Ok("1"),
	)?;

	app.input_profile_override = options
		.input_profile
		.as_deref()
		.map(load_input_profile)
		.transpose()?;

	app.smoke_test = options.smoke_test;
	if app.smoke_test {
		suppress_smoke_error_dialogs();
	}
	app.standalone_runtime = options.mode == LaunchMode::Player;
	if app.standalone_runtime
		&& let Some(project) = &app.project
	{
		progress::resolve(project)?;
	}
	engine_logger::debug!(target: "game", "Launch configuration selected: mode={:?}, installed={}", options.mode, app.installed_project);

	// Validate launch configuration before creating platform/GPU resources.
	let event_loop = EventLoop::new().expect("failed to create event loop");
	event_loop.set_control_flow(ControlFlow::Wait);

	engine_logger::component_health("game", "HEALTHY", true, "Game event loop started");

	event_loop
		.run_app(&mut app)
		.expect("game event loop failed");

	engine_logger::component_health("game", "STOPPED", false, "Game event loop stopped");
	if let Some(error) = app.fatal_error.take() {
		return Err(error);
	}
	if app.smoke_test && !app.smoke_test_completed {
		return Err("Startup smoke test exited without completing a frame".into());
	}
	Ok(())
}

// A crashing unattended smoke process must terminate instead of opening a
// Windows Error Reporting dialog. This does not change ordinary interactive runs.
fn suppress_smoke_error_dialogs() {
	#[cfg(windows)]
	unsafe {
		#[link(name = "kernel32")]
		unsafe extern "system" {
			fn SetErrorMode(mode: u32) -> u32;
		}
		const SEM_FAILCRITICALERRORS: u32 = 0x0001;
		const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
		SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
	}
}

// Diagnostics latch only: simulation and input continue to follow their existing
// recovery policy. Error Strings can contain authored data, so never persist them.
fn report_update_result(operation: &'static str, result: Result<(), String>, reported: &mut bool) {
	if result.is_err() {
		if !*reported {
			engine_logger::error!(target: "game", "Game update failed: operation={operation}");
		}
		*reported = true;
	} else if *reported {
		*reported = false;
		engine_logger::debug!(target: "game", "Game update recovered: operation={operation}");
	}
}

fn read_legacy_project_name(path: &Path) -> Option<String> {
	const MAX_LEGACY_NAME_BYTES: u64 = 80;
	if !std::fs::symlink_metadata(path).ok()?.is_file() {
		return None;
	}
	let mut name = String::new();
	File::open(path)
		.ok()?
		.take(MAX_LEGACY_NAME_BYTES + 1)
		.read_to_string(&mut name)
		.ok()?;
	(name.len() as u64 <= MAX_LEGACY_NAME_BYTES).then_some(name)
}

fn load_input_profile(path: &Path) -> Result<InputProfile, String> {
	let file =
		File::open(path).map_err(|error| format!("Cannot open input profile {path:?}: {error}"))?;

	let metadata = file
		.metadata()
		.map_err(|error| format!("Cannot inspect input profile {path:?}: {error}"))?;

	if !metadata.is_file() {
		return Err(format!("Input profile {path:?} must be a regular file"));
	}

	let mut bytes = Vec::new();
	file.take((MAX_INPUT_PROFILE_BYTES + 1) as u64)
		.read_to_end(&mut bytes)
		.map_err(|error| format!("Cannot read input profile {path:?}: {error}"))?;

	if bytes.len() > MAX_INPUT_PROFILE_BYTES {
		return Err(format!(
			"Input profile {path:?} exceeds {MAX_INPUT_PROFILE_BYTES} bytes"
		));
	}

	let json = std::str::from_utf8(&bytes)
		.map_err(|error| format!("Input profile {path:?} is not valid UTF-8: {error}"))?;

	InputProfile::from_json(json).map_err(|error| format!("Input profile {path:?}: {error}"))
}

#[cfg(test)]
#[path = "tests_app_scene_io.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "tests_exported_run.rs"]
mod exported_run_tests;

#[cfg(all(test, windows))]
#[path = "tests_performance.rs"]
mod performance_tests;

#[cfg(all(test, windows))]
#[path = "tests_asset_reimport.rs"]
mod asset_reimport_tests;

#[cfg(all(test, windows))]
#[path = "tests_runtime_lifecycle.rs"]
mod runtime_lifecycle_tests;

#[cfg(all(test, windows))]
#[path = "tests_animation_native.rs"]
mod animation_native_tests;
