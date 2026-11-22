mod cinder_checkpoint;
mod editor_panel;
mod editor_tools;
mod feedback_audio;
mod feedback_captions;

mod frame_metrics;
mod gizmo_overlay;
mod input_preset_transfer;
mod local_player_files;
mod native_input;
mod play;
mod player_controls;
mod playtrace;
mod project_sound;
mod scene_documents;
mod scene_prefetch;
mod scene_rendering;
mod selection_overlay;

#[cfg(test)]
use scene_rendering::gameplay_hud_rows;

use editor_panel::PanelView;
use editor_tools::EditorTools;
use runtime::SceneEditor;
use runtime::scene::Scene;
use runtime::scene_load_timing::{
	Outcome as SceneLoadOutcome, SceneLoadTiming, Scope as SceneLoadScope,
};
use runtime::{Camera, CameraController};
use runtime::{FrameClock, GameContentRect};
use runtime::{InputState, Key};
use selection_overlay::SelectionOverlay;

use ::game::launch::{LaunchMode, LaunchOptions};
use engine_logger::ResultLogExt;
use player_controls::PromptDevice;
use renderer::{WindowRenderer, vulkan::CameraUniform};
use runtime::assets::AssetManager;
use runtime::camera_input::{
	CameraInputOwner, CameraIntent, WheelSample, normalize_mouse, normalize_wheel,
};
use runtime::input::{
	GamepadOwner, GamepadSample, InputProfile, MAX_INPUT_PROFILE_BYTES, MovementBindings,
};
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
	asset_manager: Option<AssetManager>,
	renderer: Option<WindowRenderer>,
	window: Option<Arc<Window>>,
	clock: FrameClock,
	input: InputState,
	play_gamepad: GamepadOwner,
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
	player_controls: player_controls::PlayerControls,
	play_trace: Option<playtrace::PlayTrace>,
	feedback_audio: feedback_audio::FeedbackAudio,
	feedback_captions: feedback_captions::FeedbackCaptions,
	project_sound: project_sound::ProjectSound,
	scene_load_timing: Option<SceneLoadTiming>,
	scene_prefetch: Option<scene_prefetch::PlayerPrefetch>,
	scene_prefetch_enabled: bool,
	transfer_directory: Option<PathBuf>,
	project_path: Option<PathBuf>,
	installed_project: bool,
	play_update_error_reported: bool,
	edit_update_error_reported: bool,
	smoke_test: bool,
	smoke_scene_cycles: u32,
	smoke_prefetch_probe: bool,
	smoke_test_completed: bool,
	fatal_error: Option<String>,
}

impl GameApp {
	fn update(&mut self, delta_seconds: f32) {
		self.update_project_sound();
		if self.standalone_runtime {
			if !self.player_controls.is_open() {
				self.update_player(delta_seconds);
			}
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

	fn update_project_sound(&mut self) {
		let focused = self
			.window
			.as_ref()
			.is_some_and(|window| window.has_focus());
		self.set_project_sound_focus(focused);
	}

	fn set_project_sound_focus(&mut self, focused: bool) {
		if !self.project_sound.has_soundtrack() {
			return;
		}
		let visible = self.window.as_ref().is_some_and(|window| {
			let size = window.inner_size();
			size.width > 0 && size.height > 0
		});
		self.project_sound.update(
			self.editor.is_playing(),
			focused && visible && !self.window_occluded,
			self.player_controls.feedback_mix().effective_percent(),
		);
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
		if self.window_occluded
			|| (self.standalone_runtime
				&& ((self.game_completed() && !self.cinder_victory_animation_active())
					|| self.player_controls.is_open()))
		{
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

	#[cfg(windows)]
	fn poll_play_gamepad(&mut self, now: Instant) -> Option<Instant> {
		if !self.standalone_runtime
			|| self.smoke_test
			|| self.window_occluded
			|| !self.editor.is_playing()
			|| !self.window.as_ref().is_some_and(|window| {
				let size = window.inner_size();
				window.has_focus() && GameContentRect::fit(size.width, size.height).is_some()
			}) {
			self.reset_gamepad_input();
			return None;
		}
		self.sample_standalone_gamepad(now, runtime::input::poll_gamepad)
	}

	#[cfg(windows)]
	fn sample_standalone_gamepad(
		&mut self,
		now: Instant,
		read: impl FnMut(u32) -> Option<runtime::input::PhysicalGamepadSample>,
	) -> Option<Instant> {
		let was_connected = self.play_gamepad.connected();
		let previous = self.logical_gamepad();
		let changed = self.play_gamepad.poll(now, read);
		let connected = self.play_gamepad.connected();
		let disconnected = was_connected && !connected;
		if disconnected {
			self.player_controls.controller_disconnected();
		}
		if connected && self.player_controls_gamepad(self.play_gamepad.sample()) {
			self.request_input_frame();
			// Opening, closing, or applying can reset the polling owner.
			return Some(self.play_gamepad.deadline().unwrap_or(now));
		}
		let changed = changed
			&& (self.editor.has_authored_input_actions() || previous != self.logical_gamepad());
		if changed {
			if let Err(error) = self.submit_play_controls() {
				engine_logger::error!(target: "game", "Controller input rejected: {error}");
				self.clear_game_controls();
				return Some(now + Duration::from_millis(16));
			}
			self.note_controller_activity(previous);
		}
		if changed || disconnected {
			self.request_input_frame();
		}
		self.play_gamepad.deadline()
	}

	fn physical_gamepad(&self) -> runtime::input::PhysicalGamepadSample {
		if self.player_controls.is_open()
			|| self.editor.simulation_paused()
			|| self.game_completed()
		{
			runtime::input::PhysicalGamepadSample::default()
		} else {
			self.play_gamepad.sample()
		}
	}

	fn logical_gamepad(&self) -> GamepadSample {
		self.player_controls
			.controller()
			.resolve(self.physical_gamepad())
	}

	fn effective_input_profile(&self) -> Option<&InputProfile> {
		self.input_profile_override
			.as_ref()
			.or_else(|| self.project.as_ref()?.input_profile.as_ref())
	}

	fn cinder_in_duel(&self) -> bool {
		self.cinder_phase() == Some(gameplay_catalog::cinder_oath::Phase::Duel)
	}

	fn cinder_phase(&self) -> Option<gameplay_catalog::cinder_oath::Phase> {
		self.editor
			.play_progress::<gameplay_catalog::cinder_oath::CinderProgress>()
			.map(gameplay_catalog::cinder_oath::CinderProgress::phase)
	}

	fn cinder_victory_animation_active(&self) -> bool {
		self.cinder_phase() == Some(gameplay_catalog::cinder_oath::Phase::Victory)
			&& self
				.editor
				.animations()
				.is_some_and(|bank| bank.has_active_runtime_one_shot())
	}

	fn pointer_attack_input(&self) -> Option<InputState> {
		if !self.standalone_runtime
			|| !self.mouse_captured
			|| !self.cinder_in_duel()
			|| !self.input.is_held(runtime::input::MouseButton::Left)
		{
			return None;
		}
		let attack = self
			.effective_input_profile()?
			.actions
			.iter()
			.find(|binding| binding.id == "attack")?;
		let mut input = self.input.clone();
		input.press(attack.key);
		Some(input)
	}

	fn submit_play_controls(&mut self) -> Result<(), String> {
		let pad = self.logical_gamepad();
		let physical = self.physical_gamepad();
		let pointer_input = self.pointer_attack_input();
		let input = pointer_input.as_ref().unwrap_or(&self.input);
		self.editor
			.sample_play_action_controls(input, pad, physical)?;
		if let Some(trace) = self.play_trace.as_mut() {
			if let Some(map) = self.editor.play_input_action_map() {
				trace.record_input_actions(map.held_keyboard_codes(input), physical);
			} else {
				trace.record_controls(input, pad);
			}
		}
		Ok(())
	}

	#[cfg(windows)]
	fn note_controller_activity(&mut self, previous: GamepadSample) {
		let pad = self.logical_gamepad();
		let moving = pad.movement() != [0.0; 2]
			&& [pad.left_x, pad.left_y] != [previous.left_x, previous.left_y]
			&& self.editor.play_movement_bindings().direction(&self.input) == [0.0; 2];
		let action = self.effective_input_profile().is_some_and(|profile| {
			profile.actions.iter().any(|binding| {
				!self.input.is_held(binding.key)
					&& match binding.id.as_str() {
						"jump" => pad.jump && !previous.jump,
						"interact" => pad.interact && !previous.interact,
						_ => false,
					}
			})
		});
		if moving || action {
			self.player_controls
				.set_prompt_device(PromptDevice::Controller);
		}
	}

	fn clear_game_controls(&mut self) {
		self.editor.clear_play_input();
		self.reset_gamepad_input();
		self.player_controls
			.set_prompt_device(PromptDevice::Keyboard);
		if let Some(trace) = self.play_trace.as_mut() {
			trace.clear_controls();
		}
	}

	fn reset_gamepad_input(&mut self) {
		self.play_gamepad.reset();
		#[cfg(windows)]
		self.player_controls.reset_controller_menu_input();
	}

	fn handle_scene_commands(&mut self) -> bool {
		// The first pressed command wins. Reload precedes save to protect disk edits.
		let command = [
			(Key::F9, SceneCommand::Reload),
			(Key::F5, SceneCommand::Save),
			(Key::F6, SceneCommand::TogglePlay),
			(Key::Delete, SceneCommand::DeleteSelected),
			(Key::C, SceneCommand::DuplicateSelected),
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
			SceneCommand::DeleteSelected | SceneCommand::DuplicateSelected
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

		let asset_root = if self.installed_project {
			let assets = std::env::current_exe()
				.map_err(|e| e.to_string())?
				.parent()
				.ok_or("executable has no parent")?
				.join("assets");
			let metadata = std::fs::symlink_metadata(&assets)
				.map_err(|error| format!("Cannot inspect installed assets: {error}"))?;
			if !metadata.is_dir() || play::is_linked_package_entry(&metadata) {
				return Err("Installed assets must be an ordinary directory".into());
			}
			assets
		} else {
			self.project_path
				.as_deref()
				.and_then(Path::parent)
				.ok_or("Source project has no parent directory")?
				.join("assets")
		};
		self.initialize_window(window, asset_root)
	}

	fn initialize_window(
		&mut self,
		window: Arc<Window>,
		asset_root: PathBuf,
	) -> Result<(), String> {
		let audio_catalog = bridge::feedback_audio::load(&asset_root)?;
		let project_audio = bridge::project_audio::load(&asset_root)?;
		let size = window.inner_size();
		let project = self
			.project
			.clone()
			.ok_or("Project configuration is not initialized")?;
		let scene_path = play::resolve_scene_path(&asset_root, project.startup_scene_path()?)?;
		let parsed = scene_io::load_data(&scene_path)?;
		if let Some(timing) = &mut self.scene_load_timing {
			timing.cpu_ready();
			timing.gpu_started();
		}
		let renderer = WindowRenderer::new(Arc::clone(&window), size.width, size.height)?;
		let mut loaded = scene_io::load_parsed(&parsed, &asset_root, &renderer)?;
		let mut editor = SceneEditor::for_project(&loaded.scene, &loaded.camera, project.clone())?;
		editor.set_module_resolver(Arc::new(gameplay_catalog::resolve))?;
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

		let prefetch = if self.scene_prefetch_enabled {
			scene_prefetch::PlayerPrefetch::new(&project, &asset_root)?
		} else {
			None
		};
		if let Some(catalog) = &audio_catalog {
			catalog.mix().validate_scenes(&project.scenes)?;
		}
		let feedback_audio = feedback_audio::FeedbackAudio::new(
			audio_catalog,
			!self.smoke_test,
			self.player_controls.feedback_mix(),
			&project.startup_scene_id,
		)?;
		// Install only after every fallible initialization step has succeeded.
		if self.standalone_runtime {
			editor.start_play(&mut loaded.scene)?;
		}
		self.scene = loaded.scene;
		self.scene_prefetch = prefetch;
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
		self.feedback_audio = feedback_audio;
		self.project_sound = project_sound::ProjectSound::new(project_audio, !self.smoke_test);
		self.scene_path = Some(scene_path);
		self.renderer = Some(renderer);
		self.window = Some(window);
		self.update_project_sound();
		if let Some(timing) = &mut self.scene_load_timing {
			timing.installed();
		}
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

	fn wait_for_gpu(&self) -> Result<(), String> {
		if let Some(renderer) = self.renderer.as_ref() {
			renderer.wait_idle()?;
		}
		Ok(())
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
			_ => Err("Expected an entity editing command".to_string()),
		}
	}

	fn pick_entity(&mut self) -> Result<(), String> {
		if self.standalone_runtime || self.authored.is_some() || self.mouse_captured {
			return Ok(());
		}
		let Some((pixel, extent)) = self.cursor_geometry()? else {
			return Ok(());
		};
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
				self.clear_game_controls();
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
		self.scene_load_timing = Some(SceneLoadTiming::new(
			"player",
			SceneLoadScope::Startup,
			Instant::now(),
		));
		if let Err(error) = self
			.initialize(event_loop)
			.map_err(std::io::Error::other)
			.log_error_for("game::renderer", "initialize game")
		{
			if let Some(timing) = self.scene_load_timing.take() {
				timing.finish(SceneLoadOutcome::Failed);
			}
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
						return self.exercise_scene_loads(self.smoke_scene_cycles);
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
				if self.player_controls.is_open() {
					if state == ElementState::Released {
						self.input.release(native_input::mouse_button(button));
					}
					return;
				}
				if state == ElementState::Pressed
					&& !self.mouse_captured
					&& (self.standalone_runtime || self.editor.is_playing())
				{
					match self.cursor_geometry() {
						Ok(Some(_)) => {}
						Ok(None) => return,
						Err(error) => {
							self.report_editor_error(error);
							return;
						}
					}
				}
				// Record first so editor mode/property changes can suppress the click.
				let input_button = native_input::mouse_button(button);
				match state {
					ElementState::Pressed => self.input.press(input_button),
					ElementState::Released => self.input.release(input_button),
				}
				match (button, state) {
					(MouseButton::Left, _) if self.standalone_runtime && self.cinder_in_duel() => {
						if state == ElementState::Pressed {
							self.set_mouse_capture(true);
						}
						if let Err(error) = self.submit_play_controls() {
							self.report_editor_error(error);
						}
					}
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
				if let Ok(Some((pixel, _))) = self.cursor_geometry() {
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
				if self.standalone_runtime
					&& event.state == ElementState::Pressed
					&& (self.window_occluded
						|| !self.window.as_ref().is_some_and(|window| {
							let size = window.inner_size();
							window.has_focus() && size.width > 0 && size.height > 0
						})) {
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

				if self.standalone_runtime {
					if event.state == ElementState::Pressed
						&& !event.repeat && self.player_controls_key(key)
					{
						self.request_input_frame();
						return;
					}
					if self.player_controls.is_open() {
						if event.state == ElementState::Released {
							self.input.release(key);
						}
						return;
					}
				}
				self.request_input_frame();

				match event.state {
					ElementState::Pressed if !event.repeat => self.input.press(key),
					ElementState::Pressed => {}
					ElementState::Released => self.input.release(key),
				}
				match self.submit_play_controls() {
					Err(error) => {
						engine_logger::error!(target: "game", "Gameplay input rejected: {error}")
					}
					Ok(()) => {
						if self.standalone_runtime
							&& !self.editor.simulation_paused()
							&& self.input.just_pressed(key)
							&& event.state == ElementState::Pressed
							&& !event.repeat && self.editor.play_input_binds_key(key)
						{
							self.player_controls
								.set_prompt_device(PromptDevice::Keyboard);
						}
					}
				}
			}
			WindowEvent::Focused(false) => {
				self.set_project_sound_focus(false);
				self.player_controls.cancel_action_key_capture();
				if !self.standalone_runtime {
					self.cancel_drag();
					self.tools.panel.edit = None;
				}
				self.set_mouse_capture(false);
				self.input.suppress_held();
				self.clear_game_controls();
				self.clock.reset();

				self.next_frame_deadline = None;
			}
			WindowEvent::Focused(true) => {
				self.set_project_sound_focus(true);
				self.clock.reset();

				self.next_frame_deadline = None;
				self.window.as_ref().unwrap().request_redraw();
			}
			WindowEvent::Occluded(occluded) => {
				self.window_occluded = occluded;
				self.update_project_sound();
				self.clock.reset();

				self.next_frame_deadline = None;
				if occluded {
					self.player_controls.cancel_action_key_capture();
					self.set_mouse_capture(false);
					self.input.suppress_held();
					self.clear_game_controls();
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
					self.set_project_sound_focus(false);
					self.player_controls.cancel_action_key_capture();
					if !self.standalone_runtime {
						self.cancel_drag();
					}
					self.set_mouse_capture(false);
					self.input.suppress_held();
					self.clear_game_controls();
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
		let now = Instant::now();
		#[cfg(windows)]
		let controller_deadline = self.poll_play_gamepad(now);
		#[cfg(not(windows))]
		let controller_deadline: Option<Instant> = None;
		let Some(window) = self.window.as_ref() else {
			event_loop.set_control_flow(ControlFlow::Wait);
			return;
		};
		let size = window.inner_size();
		let frame_deadline =
			if !self.needs_continuous_redraw() || size.width == 0 || size.height == 0 {
				self.next_frame_deadline = None;
				None
			} else {
				match self.next_frame_deadline {
					Some(deadline) if deadline > now => Some(deadline),
					_ => {
						// Input and resize handlers can still request an immediate
						// frame. Only continuously scheduled redraws use this cap.
						window.request_redraw();
						None
					}
				}
			};
		if self.project_sound.poll_preview(now) {
			window.request_redraw();
		}
		let caption_expired = self.feedback_captions.expire(now);
		let caption_deadline = if self.standalone_runtime
			&& self.player_controls.display().cue_captions
			&& !self.player_controls.is_open()
			&& !self.window_occluded
			&& size.width > 0
			&& size.height > 0
		{
			if caption_expired {
				window.request_redraw();
			}
			self.feedback_captions.next_expiry()
		} else {
			None
		};
		let deadline = frame_deadline
			.into_iter()
			.chain(controller_deadline)
			.chain(caption_deadline)
			.chain(self.project_sound.preview_deadline())
			.min();
		event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
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
					self.asset_manager.take(),
				))
				.map_err(std::io::Error::other)
				.log_error_for("game::renderer", "wait before releasing scene");
		}
	}
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
	let has_manifest = match std::fs::symlink_metadata(&installed_config) {
		Ok(metadata) if metadata.is_file() && !play::is_linked_package_entry(&metadata) => true,
		Ok(_) => return Err("Installed project manifest must be an ordinary file".into()),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
		Err(error) => {
			return Err(format!(
				"Cannot inspect installed project manifest: {error}"
			));
		}
	};
	if !has_manifest && marker.is_file() {
		return Err("The installed Player is missing project.json".into());
	}
	app.installed_project = has_manifest;
	let project_path = if has_manifest {
		installed_config
	} else {
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../..")
			.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
			.join("project.json")
	};
	let project = ProjectData::load(&project_path)?;
	app.project_name = Some(project.name.clone());
	app.project_path = Some(project_path);
	app.project = Some(project);
	let arguments = std::env::args_os()
		.skip(1)
		.map(|argument| {
			argument
				.into_string()
				.map_err(|_| "Game arguments must contain valid Unicode".to_owned())
		})
		.collect::<Result<Vec<_>, _>>()?;
	let options = LaunchOptions::parse(arguments, app.installed_project)?;

	if let Some(command) = options.input_preset {
		return input_preset_transfer::execute(
			command,
			app.project_path.as_deref(),
			app.project.as_ref(),
		);
	}
	app.input_profile_override = options
		.input_profile
		.as_deref()
		.map(load_input_profile)
		.transpose()?;

	app.transfer_directory = options.transfer_directory;
	app.smoke_test = options.smoke_test;
	app.smoke_scene_cycles = options.smoke_scene_cycles;
	app.smoke_prefetch_probe = options.smoke_prefetch_probe;
	app.scene_prefetch_enabled = options.scene_prefetch;
	if app.smoke_test {
		suppress_smoke_error_dialogs();
	}
	app.standalone_runtime = options.mode == LaunchMode::Player;
	if app.standalone_runtime && !app.smoke_test {
		let authored = app
			.project
			.as_ref()
			.and_then(|project| project.input_profile.as_ref());
		if let Some(profile) = app.player_controls.configure(
			app.project_path.as_deref(),
			authored,
			app.input_profile_override.is_some(),
		) {
			app.input_profile_override = Some(profile);
		}
	}
	if app.standalone_runtime
		&& let Some(project) = &app.project
	{
		gameplay_catalog::resolve(project)?;
	}
	if let Some(path) = options.record_playtrace {
		let project = app
			.project
			.as_ref()
			.ok_or("Playtrace requires an authored project")?;
		let defaults = InputProfile::default();
		let authored = project.input_profile.as_ref().unwrap_or(&defaults);
		app.play_trace = Some(playtrace::PlayTrace::new(
			path,
			authored,
			app.effective_input_profile().unwrap_or(&defaults),
		)?);
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
	if let Some(trace) = app.play_trace.take() {
		trace.finish()?;
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

	let profile = InputProfile::from_json(json)
		.map_err(|error| format!("Input profile {path:?}: {error}"))?;
	player_controls::validate_profile(&profile)
		.map_err(|error| format!("Input profile {path:?}: {error}"))?;
	Ok(profile)
}

#[cfg(test)]
#[path = "../tests/unit/tests_app_scene_io.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "../tests/unit/tests_performance.rs"]
mod performance_tests;

#[cfg(all(test, windows))]
#[path = "../tests/unit/tests_asset_reimport.rs"]
mod asset_reimport_tests;

#[cfg(all(test, windows))]
#[path = "../tests/unit/tests_runtime_lifecycle.rs"]
mod runtime_lifecycle_tests;

#[cfg(all(test, windows))]
#[path = "../tests/unit/tests_animation_native.rs"]
mod animation_native_tests;

#[cfg(all(test, windows))]
#[path = "../tests/unit/tests_camera_native.rs"]
mod camera_native_tests;
