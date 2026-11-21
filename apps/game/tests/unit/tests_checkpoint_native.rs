//! Save and Continue through the release fixture in two fresh native processes.
use super::*;
use courtyard_gameplay::checkpoint::ContinueAvailability;
use std::{
	fs,
	os::windows::process::CommandExt,
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};
use winit::platform::windows::EventLoopBuilderExtWindows;

const PHASE_VAR: &str = "GAME_ENGINE_QA_CHECKPOINT_PHASE";
const PRIOR_PROJECT_VAR: &str = "GAME_ENGINE_QA_PRIOR_PROJECT";
const CURRENT_PROJECT_VAR: &str = "GAME_ENGINE_QA_PROJECT";
const UNAPPROVED_PROJECT_VAR: &str = "GAME_ENGINE_QA_UNAPPROVED_PROJECT";
const RELEASE_SAVE_ID: &str = "c70545a2-181c-42d0-906e-87fb32547914";
const STEP: f32 = 1.0 / 60.0;

struct ProfileDir(PathBuf);

impl ProfileDir {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"game-native-checkpoint-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&root).unwrap();
		Self(root.canonicalize().unwrap())
	}
}

impl Drop for ProfileDir {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
#[ignore = "requires the release fixture, a native window, and Vulkan"]
fn courtyard_checkpoint_continues_across_native_processes() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	let profile = ProfileDir::new();
	for phase in ["save", "continue"] {
		run_isolated_phase(
			"checkpoint_native_tests::courtyard_checkpoint_continues_across_native_processes",
			phase,
			&profile,
			None,
		);
	}
}

#[test]
#[ignore = "requires a native Windows Vulkan session and copied prior/current project manifests"]
fn save_package_upgrade_preserves_old_bytes_until_explicit_save() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"checkpoint_native_tests::save_package_upgrade_preserves_old_bytes_until_explicit_save";
	let prior = std::env::var_os(PRIOR_PROJECT_VAR)
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_PRIOR_PROJECT to the prior copied project.json");
	let current = std::env::var_os(CURRENT_PROJECT_VAR)
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_PROJECT to the updated copied project.json");
	let unapproved = std::env::var_os(UNAPPROVED_PROJECT_VAR).map(PathBuf::from);
	let profile = ProfileDir::new();
	let slot = profile
		.0
		.join("GameEngine/checkpoints")
		.join(RELEASE_SAVE_ID)
		.join("slot.json");
	run_isolated_phase(TEST, "save", &profile, Some(&prior));
	let old_bytes = fs::read(&slot).unwrap();
	run_isolated_phase(
		TEST,
		"deny",
		&profile,
		Some(unapproved.as_deref().unwrap_or(&current)),
	);
	assert_eq!(fs::read(&slot).unwrap(), old_bytes);
	run_isolated_phase(TEST, "upgrade-continue", &profile, Some(&current));
	assert_eq!(fs::read(&slot).unwrap(), old_bytes);
	run_isolated_phase(TEST, "upgrade-save", &profile, Some(&current));
	let upgraded = fs::read(&slot).unwrap();
	assert_ne!(upgraded, old_bytes);
	run_isolated_phase(TEST, "continue", &profile, Some(&current));
	assert_eq!(fs::read(slot).unwrap(), upgraded);
}

fn run_isolated_phase(test: &str, phase: &str, profile: &ProfileDir, manifest: Option<&Path>) {
	let mut command = Command::new(std::env::current_exe().unwrap());
	command
		.args(["--ignored", "--exact", test, "--nocapture"])
		.env(PHASE_VAR, phase)
		.env("LOCALAPPDATA", &profile.0)
		.env("GAME_ENGINE_QA_CHECKPOINT_PROFILE", &profile.0)
		.creation_flags(0x0800_0000);
	if let Some(manifest) = manifest {
		command.env(CURRENT_PROJECT_VAR, manifest);
	}
	let output = command.output().unwrap();
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert!(
		output.status.success() && stdout.contains(&format!("NATIVE_CHECKPOINT_OK {phase}")),
		"Native {phase}: {}\n{stdout}\n{}",
		output.status,
		String::from_utf8_lossy(&output.stderr),
	);
	print!("{stdout}");
}

struct NativePhase {
	phase: String,
	completed: bool,
}

impl ApplicationHandler for NativePhase {
	#[allow(clippy::field_reassign_with_default)]
	fn resumed(&mut self, events: &ActiveEventLoop) {
		let manifest = std::env::var_os(CURRENT_PROJECT_VAR)
			.map(PathBuf::from)
			.unwrap_or_else(|| {
				PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.release-learning.json")
			})
			.canonicalize()
			.unwrap();
		let source = fs::read(&manifest).unwrap();
		let mut project = ProjectData::load(&manifest).unwrap();
		if self.phase == "deny" && std::env::var_os(UNAPPROVED_PROJECT_VAR).is_none() {
			// Exercise an unrelated content fingerprint when a third package isn't supplied.
			project.name.push_str(" (unapproved)");
		}
		let checkpoints =
			courtyard_gameplay::checkpoint::CheckpointRegistry::from_project(&project)
				.unwrap()
				.unwrap();
		let mut controls = player_controls::PlayerControls::default();
		let input_profile_override = controls.configure(
			Some(&manifest),
			Some(checkpoints.project_save_id()),
			project.input_profile.as_ref(),
			false,
		);
		let assets = manifest.parent().unwrap().join("assets");
		let window = Arc::new(
			events
				.create_window(
					Window::default_attributes()
						.with_visible(false)
						.with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
				)
				.unwrap(),
		);
		let mut app = GameApp::default();
		app.project_name = Some(project.name.clone());
		app.project_path = Some(manifest.clone());
		app.project = Some(project);
		app.checkpoints = Some(checkpoints);
		app.player_controls = controls;
		app.input_profile_override = input_profile_override;
		app.standalone_runtime = true;
		app.installed_project = true;
		app.initialize_window(window, assets).unwrap();
		assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
		match self.phase.as_str() {
			"save" => take_checkpoint(&mut app),
			"continue" => restore_and_finish(&mut app),
			"upgrade-continue" => resume_earlier_slot(&mut app),
			"upgrade-save" => {
				resume_earlier_slot(&mut app);
				assert_eq!(app.save_checkpoint().unwrap(), "Saved after_gate");
				assert_eq!(app.checkpoint_availability(), ContinueAvailability::Current);
			}
			"deny" => reject_unlisted_slot(&mut app),
			_ => panic!("Unknown checkpoint verification phase"),
		}
		assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
		app.wait_for_gpu().unwrap();
		assert_eq!(fs::read(&manifest).unwrap(), source);
		drop(app);
		self.completed = true;
		events.exit();
	}

	fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn run_native_phase(phase: &str) {
	let local = std::env::var_os("LOCALAPPDATA").expect("isolated profile");
	let isolated =
		std::env::var_os("GAME_ENGINE_QA_CHECKPOINT_PROFILE").expect("isolated test profile");
	assert_eq!(local, isolated);
	let mut log_config = engine_logger::Config::new("native-checkpoint-test");
	log_config.console = false;
	let _logging = engine_logger::init(log_config).unwrap();
	let events = EventLoop::builder().with_any_thread(true).build().unwrap();
	let mut test = NativePhase {
		phase: phase.into(),
		completed: false,
	};
	events.run_app(&mut test).unwrap();
	assert!(test.completed);
	println!("NATIVE_CHECKPOINT_OK {phase}");
}

fn keys(app: &mut GameApp, key: Option<Key>) {
	app.input.reset();
	if let Some(key) = key {
		app.input.press(key);
	}
	app.editor.sample_play_keyboard(&app.input).unwrap();
}

fn move_for(app: &mut GameApp, key: Key, ticks: u32) {
	keys(app, Some(key));
	for _ in 0..ticks {
		app.tick_play(STEP).unwrap();
		app.input.end_frame();
	}
	keys(app, None);
}

fn progress(app: &GameApp) -> &progress::CampaignProgress {
	app.editor.play_progress().unwrap()
}

fn take_checkpoint(app: &mut GameApp) {
	assert_eq!(progress(app).score, 0);
	menu_keys(
		app,
		&[Key::Escape, Key::ArrowRight, Key::ArrowDown, Key::Enter],
	);
	assert_eq!(
		app.editor.play_movement_bindings(),
		&runtime::input::MovementBindings::ARROWS
	);
	move_for(app, Key::ArrowRight, 90);
	assert!(progress(app).has_courtyard_key);
	move_for(app, Key::ArrowLeft, 84);
	assert!(!progress(app).route_gate_open);
	// A press and release between two render frames still commits its edge.
	keys(app, Some(Key::E));
	keys(app, None);
	app.tick_play(STEP).unwrap();
	assert!(progress(app).route_gate_open);
	move_for(app, Key::ArrowLeft, 64);
	menu_keys(
		app,
		&[Key::Escape, Key::ArrowDown, Key::ArrowDown, Key::Enter],
	);
	let saved = app.player_controls.rows().unwrap().join("\n");
	assert!(saved.contains("after_gate"), "{saved}");
	menu_keys(app, &[Key::Escape]);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
}

fn restore_and_finish(app: &mut GameApp) {
	assert_eq!(app.checkpoint_availability(), ContinueAvailability::Current);
	assert_eq!(progress(app).score, 0);
	assert!(!progress(app).route_gate_open);
	assert_eq!(
		app.editor.play_movement_bindings(),
		&runtime::input::MovementBindings::ARROWS
	);
	let before = app.editor.play_status().unwrap().run_id;
	menu_keys(
		app,
		&[
			Key::Escape,
			Key::ArrowDown,
			Key::ArrowDown,
			Key::ArrowDown,
			Key::Enter,
		],
	);
	let continued = app.player_controls.rows().unwrap().join("\n");
	assert!(continued.contains("after_gate"), "{continued}");
	menu_keys(app, &[Key::Escape]);
	assert_ne!(app.editor.play_status().unwrap().run_id, before);
	assert!(progress(app).has_courtyard_key && progress(app).route_gate_open);
	assert_eq!(progress(app).score, 1);
	let document = app.editor.play_document().unwrap();
	let gate = document
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-route-gate")
		.unwrap();
	assert!(!gate.renderable && !gate.collision_enabled);
	let actor = document
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-player")
		.unwrap();
	assert_eq!(actor.transform.position, [-0.97, 0.0, 0.0]);
	move_for(app, Key::ArrowLeft, 30);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
	move_for(app, Key::ArrowRight, 90);
	assert!(progress(app).completed);
	app.restart_player_game().unwrap();
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
	assert_eq!(progress(app).score, 0);
	assert!(!progress(app).has_courtyard_key && !progress(app).route_gate_open);
}

fn resume_earlier_slot(app: &mut GameApp) {
	assert_eq!(
		app.checkpoint_availability(),
		ContinueAvailability::CompatibleEarlierRelease
	);
	let before = app.editor.play_status().unwrap().run_id;
	menu_keys(
		app,
		&[Key::Escape, Key::ArrowDown, Key::ArrowDown, Key::ArrowDown],
	);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.join("\n")
			.contains("> Continue (Earlier release)")
	);
	menu_keys(app, &[Key::Enter]);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.join("\n")
			.contains("Loaded earlier save (after_gate)")
	);
	assert_ne!(app.editor.play_status().unwrap().run_id, before);
	assert!(progress(app).has_courtyard_key && progress(app).route_gate_open);
	assert_eq!(progress(app).score, 1);
	let document = app.editor.play_document().unwrap();
	let player = document
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-player")
		.unwrap();
	assert_eq!(player.transform.position, [-0.97, 0.0, 0.0]);
	let gate = document
		.entities
		.iter()
		.find(|entity| entity.key.as_str() == "courtyard-route-gate")
		.unwrap();
	assert!(!gate.renderable && !gate.collision_enabled);
	menu_keys(app, &[Key::Escape]);
}

fn reject_unlisted_slot(app: &mut GameApp) {
	let before = app.editor.play_status().unwrap().run_id;
	assert_eq!(
		app.checkpoint_availability(),
		ContinueAvailability::IncompatibleRelease
	);
	menu_keys(
		app,
		&[
			Key::Escape,
			Key::ArrowDown,
			Key::ArrowDown,
			Key::ArrowDown,
			Key::Enter,
		],
	);
	assert!(
		app.player_controls
			.rows()
			.unwrap()
			.join("\n")
			.contains("> Continue (Incompatible with this release)")
	);
	assert_eq!(app.editor.play_status().unwrap().run_id, before);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
	assert_eq!(progress(app).score, 0);
	assert!(!progress(app).has_courtyard_key && !progress(app).route_gate_open);
}

fn menu_keys(app: &mut GameApp, keys: &[Key]) {
	for key in keys {
		assert!(app.player_controls_key(*key));
	}
}
