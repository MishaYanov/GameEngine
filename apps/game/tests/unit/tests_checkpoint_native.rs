//! Save and Continue through the release fixture in two fresh native processes.
use super::*;
use courtyard_gameplay::checkpoint::{ContinueAvailability, SaveSlot};
use std::{
	fs,
	os::windows::process::CommandExt,
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};
use winit::platform::windows::EventLoopBuilderExtWindows;

#[path = "tests_checkpoint_transfer_native.rs"]
mod checkpoint_transfer_native;

pub(super) const PHASE_VAR: &str = "GAME_ENGINE_QA_CHECKPOINT_PHASE";
const PRIOR_PROJECT_VAR: &str = "GAME_ENGINE_QA_PRIOR_PROJECT";
pub(super) const CURRENT_PROJECT_VAR: &str = "GAME_ENGINE_QA_PROJECT";
const UNAPPROVED_PROJECT_VAR: &str = "GAME_ENGINE_QA_UNAPPROVED_PROJECT";
const RELEASE_SAVE_ID: &str = "c70545a2-181c-42d0-906e-87fb32547914";
const STEP: f32 = 1.0 / 60.0;

pub(super) struct ProfileDir(pub(super) PathBuf);

impl ProfileDir {
	pub(super) fn new() -> Self {
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

#[test]
#[ignore = "requires the release fixture, a native Windows session and Vulkan"]
fn courtyard_slots_restore_across_processes_and_offer_explicit_recovery() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str = "checkpoint_native_tests::courtyard_slots_restore_across_processes_and_offer_explicit_recovery";
	let profile = ProfileDir::new();
	let directory = profile
		.0
		.join("GameEngine/checkpoints")
		.join(RELEASE_SAVE_ID);
	run_isolated_phase(TEST, "slots-save", &profile, None);
	let slot_one = directory.join("slot.json");
	let previous_one = directory.join("slot.previous.json");
	let slot_two = directory.join("slot-2.json");
	let saved_two = fs::read(&slot_two).unwrap();
	let previous = fs::read(&previous_one).unwrap();
	assert_ne!(fs::read(&slot_one).unwrap(), previous);
	let corrupt = b"simulated interrupted checkpoint";
	fs::write(&slot_one, corrupt).unwrap();
	run_isolated_phase(TEST, "slots-restore", &profile, None);
	assert_eq!(fs::read(slot_one).unwrap(), corrupt);
	assert_eq!(fs::read(previous_one).unwrap(), previous);
	assert_eq!(fs::read(slot_two).unwrap(), saved_two);
	assert!(!directory.join("slot-3.json").exists());
}

pub(super) fn run_isolated_phase(
	test: &str,
	phase: &str,
	profile: &ProfileDir,
	manifest: Option<&Path>,
) {
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

#[test]
#[ignore = "requires the chaptered fixture, a native Windows session and Vulkan"]
fn chapter_slots_continue_and_recover_across_native_processes() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"checkpoint_native_tests::chapter_slots_continue_and_recover_across_native_processes";
	let manifest = std::env::var_os("GAME_ENGINE_QA_CHAPTER_PROJECT")
		.map(PathBuf::from)
		.unwrap_or_else(|| {
			PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.courtyard-chapters.json")
		});
	let profile = ProfileDir::new();
	let directory = profile
		.0
		.join("GameEngine/checkpoints/3ef795f1-4bb8-41ff-bebb-66a170e68878");
	run_isolated_phase(TEST, "chapters-save", &profile, Some(&manifest));
	let one = directory.join("slot.json");
	let two = directory.join("slot-2.json");
	let previous = directory.join("slot-2.previous.json");
	let saved_courtyard = fs::read(&one).unwrap();
	let saved_garden = fs::read(&two).unwrap();
	let previous_garden = fs::read(&previous).unwrap();
	assert_ne!(saved_garden, previous_garden);
	run_isolated_phase(TEST, "chapters-continue", &profile, Some(&manifest));
	assert_eq!(fs::read(&two).unwrap(), saved_garden);
	let incomplete = b"{\"saved\":";
	fs::write(&two, incomplete).unwrap();
	run_isolated_phase(TEST, "chapters-recover", &profile, Some(&manifest));
	assert_eq!(fs::read(one).unwrap(), saved_courtyard);
	assert_eq!(fs::read(two).unwrap(), incomplete);
	assert_eq!(fs::read(previous).unwrap(), previous_garden);
}

#[test]
#[ignore = "requires native Windows Vulkan and a chaptered fixture with a version 2 feedback catalog"]
fn chapter_feedback_mix_follows_scene_commit_and_checkpoint_restore() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"checkpoint_native_tests::chapter_feedback_mix_follows_scene_commit_and_checkpoint_restore";
	let manifest = std::env::var_os("GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT")
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT to the chaptered fixture project.json");
	let profile = ProfileDir::new();
	run_isolated_phase(TEST, "chapters-audio-save", &profile, Some(&manifest));
	run_isolated_phase(TEST, "chapters-audio-continue", &profile, Some(&manifest));
}

#[test]
#[ignore = "requires native Windows Vulkan and a chaptered fixture with a version 2 feedback catalog"]
fn local_feedback_categories_persist_through_scene_changes_and_restore() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str = "checkpoint_native_tests::local_feedback_categories_persist_through_scene_changes_and_restore";
	let manifest = std::env::var_os("GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT")
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT to the chaptered fixture project.json");
	let profile = ProfileDir::new();
	for phase in [
		"chapters-audio-levels-save",
		"chapters-audio-levels-continue",
	] {
		run_isolated_phase(TEST, phase, &profile, Some(&manifest));
	}
}

#[test]
#[ignore = "requires native Windows Vulkan and a chaptered fixture with a version 2 feedback catalog"]
fn feedback_diagnostics_and_auditions_use_the_committed_garden_mix() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"checkpoint_native_tests::feedback_diagnostics_and_auditions_use_the_committed_garden_mix";
	let manifest = std::env::var_os("GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT")
		.map(PathBuf::from)
		.expect("set GAME_ENGINE_QA_CHAPTER_AUDIO_PROJECT to the chaptered fixture project.json");
	let profile = ProfileDir::new();
	for phase in ["chapters-audio-levels-save", "chapters-audio-audition"] {
		run_isolated_phase(TEST, phase, &profile, Some(&manifest));
	}
	println!(
		"{}",
		fs::read_to_string(profile.0.join("cue-audition-evidence.txt")).unwrap()
	);
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
		app.smoke_test =
			self.phase.starts_with("chapters-audio-") && self.phase != "chapters-audio-audition";
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
			"slots-save" => save_independent_slots(&mut app),
			"slots-restore" => restore_independent_slots(&mut app),
			"chapters-save" => save_chapter_slots(&mut app, |_| {}),
			"chapters-audio-save" | "chapters-audio-continue" => {
				check_chapter_audio(&mut app, &self.phase);
			}
			"chapters-audio-levels-save" | "chapters-audio-levels-continue" => {
				check_player_bus_levels(&mut app, self.phase == "chapters-audio-levels-save");
			}
			"chapters-audio-audition" => check_player_cue_diagnostics(&mut app),
			"chapters-transfer-continue" => {
				checkpoint_transfer_native::continue_imported_garden(&mut app)
			}
			"chapters-transfer-menu-export"
			| "chapters-transfer-menu-import"
			| "chapters-transfer-history-manage"
			| "chapters-transfer-history-relaunch" => {
				checkpoint_transfer_native::run_menu_phase(&mut app, &self.phase)
			}
			"chapters-continue" => continue_chapter_slots(&mut app, false),
			"chapters-recover" => continue_chapter_slots(&mut app, true),
			"controller-menu-save" | "controller-menu-continue" => {
				super::controller_menu_native_tests::run_phase(&mut app, &self.phase)
			}
			"trace-remap"
			| "trace-record"
			| "trace-keyboard-remap"
			| "trace-keyboard-verify"
			| "trace-keyboard-record"
			| "preset-export"
			| "preset-apply"
			| "preset-relaunch"
			| "preset-library-save"
			| "preset-library-review"
			| "preset-library-relaunch"
			| "preset-library-backup-export"
			| "preset-library-backup-restore"
			| "preset-library-backup-relaunch" => {
				super::playtrace_native_tests::run_phase(&mut app, &self.phase)
			}
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

pub(super) fn run_native_phase(phase: &str) {
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

pub(super) fn move_for(app: &mut GameApp, key: Key, ticks: u32) {
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
	open_slot_actions(app, true, SaveSlot::One);
	menu_keys(app, &[Key::Enter]);
	let saved = app.player_controls.rows().unwrap().join("\n");
	assert!(saved.contains("after_gate"), "{saved}");
	close_checkpoint_menu(app);
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
	open_slot_actions(app, false, SaveSlot::One);
	menu_keys(app, &[Key::Enter]);
	let continued = app.player_controls.rows().unwrap().join("\n");
	assert!(continued.contains("after_gate"), "{continued}");
	close_checkpoint_menu(app);
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
	open_slot_actions(app, false, SaveSlot::One);
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
	close_checkpoint_menu(app);
}

fn reject_unlisted_slot(app: &mut GameApp) {
	let before = app.editor.play_status().unwrap().run_id;
	assert_eq!(
		app.checkpoint_availability(),
		ContinueAvailability::IncompatibleRelease
	);
	open_slot_actions(app, false, SaveSlot::One);
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

fn save_independent_slots(app: &mut GameApp) {
	take_checkpoint(app);
	app.restart_player_game().unwrap();

	open_slot_actions(app, true, SaveSlot::Two);
	menu_keys(app, &[Key::Enter]);
	assert_eq!(
		app.checkpoint_slot(SaveSlot::Two).checkpoint_id.as_deref(),
		Some("entry")
	);
	close_checkpoint_menu(app);

	open_slot_actions(app, true, SaveSlot::One);
	// Enter again lands on Cancel, leaving the original checkpoint intact.
	menu_keys(app, &[Key::Enter, Key::Enter]);
	assert_eq!(
		app.checkpoint_slot(SaveSlot::One).checkpoint_id.as_deref(),
		Some("after_gate")
	);
	menu_keys(app, &[Key::Enter, Key::ArrowRight, Key::Enter]);
	assert_eq!(
		app.checkpoint_slot(SaveSlot::One).checkpoint_id.as_deref(),
		Some("entry")
	);
	close_checkpoint_menu(app);
}

fn restore_independent_slots(app: &mut GameApp) {
	let before = app.editor.play_status().unwrap().run_id;
	open_slot_actions(app, false, SaveSlot::Two);
	menu_keys(app, &[Key::Enter]);
	assert_ne!(app.editor.play_status().unwrap().run_id, before);
	assert_eq!(progress(app).score, 0);
	let rows = app.player_controls.rows().unwrap().join("\n");
	assert!(rows.contains("Continued from entry"), "{rows}");
	close_checkpoint_menu(app);

	let before = app.editor.play_status().unwrap().run_id;
	open_slot_actions(app, false, SaveSlot::One);
	let rows = app.player_controls.rows().unwrap().join("\n");
	assert!(rows.contains("Recover prior"), "{rows}");
	menu_keys(app, &[Key::ArrowDown, Key::Enter]);
	assert_ne!(app.editor.play_status().unwrap().run_id, before);
	assert!(progress(app).route_gate_open);
	assert_eq!(progress(app).score, 1);
	let rows = app.player_controls.rows().unwrap().join("\n");
	assert!(rows.contains("Recovered prior save (after_gate)"), "{rows}");
}

fn open_slot_actions(app: &mut GameApp, saving: bool, slot: SaveSlot) {
	menu_keys(app, &[Key::Escape, Key::ArrowDown, Key::ArrowDown]);
	if !saving {
		menu_keys(app, &[Key::ArrowDown]);
	}
	menu_keys(app, &[Key::Enter]);
	for candidate in SaveSlot::ALL {
		if candidate == slot {
			break;
		}
		menu_keys(app, &[Key::ArrowDown]);
	}
	menu_keys(app, &[Key::Enter]);
}

fn check_chapter_audio(app: &mut GameApp, phase: &str) {
	use bridge::feedback_audio::FeedbackCue;
	let gate = FeedbackCue::GateOpened;
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
	assert_eq!(app.feedback_audio.test_gain(gate), 80);

	if phase == "chapters-audio-save" {
		let missing = SceneId::new("missing").unwrap();
		assert!(app.switch_player_scene(missing).is_err());
		assert_eq!(app.feedback_audio.test_gain(gate), 80);
		save_chapter_slots(app, |_| {});
	} else {
		open_slot_actions(app, false, SaveSlot::Two);
		menu_keys(app, &[Key::Enter]);
		close_checkpoint_menu(app);
		assert_eq!(progress(app).score, 2);
	}
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(app.feedback_audio.test_gain(gate), 40);
	if phase == "chapters-audio-continue" {
		move_for(app, Key::D, 90);
		assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
		assert_eq!(app.feedback_audio.test_gain(gate), 80);
	}
}

fn check_player_bus_levels(app: &mut GameApp, save: bool) {
	use bridge::feedback_audio::FeedbackCue;
	let gains = |app: &GameApp| FeedbackCue::ALL.map(|cue| app.feedback_audio.test_gain(cue));
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");

	if save {
		assert_eq!(gains(app), [100, 80, 100]);
		set_native_audio_categories(app);
		assert_eq!(gains(app), [0, 32, 48]);
		let missing = SceneId::new("missing").unwrap();
		assert!(app.switch_player_scene(missing).is_err());
		assert_eq!(gains(app), [0, 32, 48]);
		save_chapter_slots(app, |app| {
			assert_eq!(gains(app), [0, 16, 48]);
			assert!(
				app.feedback_captions
					.rows(Instant::now())
					.any(|row| row == "Key collected")
			);
		});
	} else {
		assert_eq!(gains(app), [0, 32, 48]);
		assert_eq!(
			app.player_controls.display().overlay_style().text_scale,
			1.5
		);
		open_slot_actions(app, false, SaveSlot::Two);
		menu_keys(app, &[Key::Enter]);
		close_checkpoint_menu(app);
		assert_eq!(progress(app).score, 2);
	}
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(gains(app), [0, 16, 48]);
	if !save {
		move_for(app, Key::D, 90);
		assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
		assert_eq!(gains(app), [0, 32, 48]);
		move_for(app, Key::D, 90);
		assert!(progress(app).completed);
		assert!(
			app.feedback_captions
				.rows(Instant::now())
				.any(|row| row == "Run completed")
		);
	}
}

fn focus_audio_control(app: &mut GameApp, label: &str, dpad: bool) {
	use runtime::input::MenuButtons;

	let prefix = format!("> {label}");
	for _ in 0..12 {
		if app.player_controls_rows(1, 22).unwrap()[0].starts_with(&prefix) {
			return;
		}
		if dpad {
			press_audio_gamepad(
				app,
				MenuButtons {
					down: true,
					..MenuButtons::default()
				},
				[false; 4],
			);
		} else {
			menu_keys(app, &[Key::ArrowDown]);
		}
	}
	panic!("Missing Controls row {label}");
}

fn press_audio_gamepad(
	app: &mut GameApp,
	menu: runtime::input::MenuButtons,
	face_buttons: [bool; 4],
) {
	use runtime::input::PhysicalGamepadSample;

	assert!(!app.player_controls_gamepad(PhysicalGamepadSample::default()));
	let sample = PhysicalGamepadSample {
		menu,
		face_buttons,
		..PhysicalGamepadSample::default()
	};
	assert!(app.player_controls_gamepad(sample));
	assert!(!app.player_controls_gamepad(sample));
}

fn check_player_cue_diagnostics(app: &mut GameApp) {
	use renderer::ui::ScreenOverlay;
	use runtime::input::MenuButtons;
	use std::{thread, time::Duration};

	open_slot_actions(app, false, SaveSlot::Two);
	menu_keys(app, &[Key::Enter]);
	close_checkpoint_menu(app);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(progress(app).score, 2);
	let garden_progress = progress(app).clone();
	let style = app.player_controls.display().overlay_style();
	assert_eq!(style.text_scale, 1.5);
	let capacity = ScreenOverlay::capacity_with_style([800, 450], 1.0, style);

	menu_keys(app, &[Key::Escape]);
	focus_audio_control(app, "Feedback Audio", false);
	menu_keys(app, &[Key::Enter, Key::ArrowLeft]);
	focus_audio_control(app, "Test Collection", false);
	menu_keys(app, &[Key::Enter]);
	assert_eq!(
		app.player_controls_rows(2, 22).unwrap()[1],
		"Cue gain is zero"
	);
	focus_audio_control(app, "Cue Details", false);
	menu_keys(app, &[Key::Enter, Key::ArrowRight]);
	for row in [
		"Category: Mechanisms",
		"Master: 80%",
		"Local: 50%",
		"Project: 80%",
		"Scene: 50%",
		"Resolved: 16%",
		"Clip: Authored",
		"Cue captions: On",
	] {
		focus_audio_control(app, row, false);
		let rows = app
			.player_controls_rows(capacity.rows, capacity.columns)
			.unwrap();
		assert!(rows.contains(&format!("> {row}")), "{rows:?}");
	}
	assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
	let diagnostic = "Garden with Extra large text: Master 80%, Local 50%, Project 80%, Scene 50%, Resolved 16%, Clip Authored, Cue captions On.";
	menu_keys(app, &[Key::Escape, Key::Escape, Key::Enter]);

	// Enable Collection after proving zero gain and checking the committed, undrafted master.
	focus_audio_control(app, "Collection", false);
	menu_keys(app, &[Key::ArrowRight]);
	focus_audio_control(app, "Apply", false);
	menu_keys(app, &[Key::Enter, Key::Enter]);

	let profile = app
		.editor
		.project()
		.unwrap()
		.input_profile
		.clone()
		.unwrap_or_default();
	let path = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
	let trace_path = path.join("cue-audition-trace.json");
	app.play_trace =
		Some(playtrace::PlayTrace::new(trace_path.clone(), &profile, &profile).unwrap());
	let mut submitted = 0;
	for controller in [false, true] {
		for label in ["Test Collection", "Test Mechanisms", "Test Completion"] {
			focus_audio_control(app, label, controller);
			if controller {
				press_audio_gamepad(app, MenuButtons::default(), [true, false, false, false]);
			} else {
				menu_keys(app, &[Key::Enter]);
			}
			assert_eq!(
				app.player_controls_rows(2, 22).unwrap()[1],
				"Cue test submitted"
			);
			submitted += 1;
			let deadline = Instant::now() + Duration::from_secs(3);
			while app.feedback_audio.test_audition_counts().0 < submitted {
				assert!(Instant::now() < deadline, "Worker did not consume {label}");
				thread::sleep(Duration::from_millis(10));
			}
		}
	}
	let (completed, played) = app.feedback_audio.test_audition_counts();
	assert_eq!(completed, 6);
	assert!(*progress(app) == garden_progress);
	assert!(app.feedback_captions.rows(Instant::now()).next().is_none());
	app.play_trace.take().unwrap().finish().unwrap();
	let trace: serde_json::Value = serde_json::from_slice(&fs::read(trace_path).unwrap()).unwrap();
	assert_eq!(trace["actions"].as_array().unwrap().len(), 1);
	menu_keys(app, &[Key::Escape, Key::Escape]);
	move_for(app, Key::D, 90);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
	move_for(app, Key::D, 90);
	assert!(progress(app).completed);
	assert!(
		app.feedback_captions
			.rows(Instant::now())
			.any(|row| row == "Run completed")
	);
	assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);

	fs::write(
		path.join("cue-audition-evidence.txt"),
		format!(
			"{diagnostic}\nKeyboard and controller submitted three tests each. Windows playback succeeded for {played} of {completed} queued tests.\nGameplay rendered the Run completed caption. Auditions left gameplay progress, captions and replay unchanged.\n"
		),
	)
		.unwrap();
}

fn set_native_audio_categories(app: &mut GameApp) {
	menu_keys(app, &[Key::Escape]);
	focus_audio_control(app, "Accessibility", false);
	menu_keys(app, &[Key::Enter, Key::ArrowRight, Key::ArrowRight]);
	focus_audio_control(app, "Apply", false);
	menu_keys(app, &[Key::Enter]);
	assert_eq!(
		app.player_controls.display().overlay_style().text_scale,
		1.5
	);

	focus_audio_control(app, "Feedback Audio", false);
	menu_keys(app, &[Key::Enter]);
	for (label, steps) in [
		("Volume", 2),
		("Collection", 10),
		("Mechanisms", 5),
		("Completion", 4),
	] {
		focus_audio_control(app, label, false);
		for _ in 0..steps {
			menu_keys(app, &[Key::ArrowLeft]);
		}
	}
	focus_audio_control(app, "Collection: 0%", false);
	assert_eq!(app.render().unwrap(), renderer::DrawStatus::Rendered);
	focus_audio_control(app, "Apply", false);
	menu_keys(app, &[Key::Enter, Key::Escape]);
	assert!(!app.player_controls.is_open());
}

fn save_chapter_slots(app: &mut GameApp, after_garden_key: impl FnOnce(&GameApp)) {
	move_for(app, Key::D, 90);
	move_for(app, Key::A, 84);
	keys(app, Some(Key::E));
	keys(app, None);
	app.tick_play(STEP).unwrap();
	move_for(app, Key::A, 64);
	open_slot_actions(app, true, SaveSlot::One);
	menu_keys(app, &[Key::Enter]);
	assert_eq!(
		app.checkpoint_slot(SaveSlot::One).chapter_label.as_deref(),
		Some("Courtyard")
	);
	close_checkpoint_menu(app);
	move_for(app, Key::A, 30);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(progress(app).score, 1);
	open_slot_actions(app, true, SaveSlot::Two);
	menu_keys(app, &[Key::Enter]);
	close_checkpoint_menu(app);
	move_for(app, Key::A, 90);
	assert_eq!(progress(app).score, 2);
	after_garden_key(app);
	move_for(app, Key::D, 90);
	open_slot_actions(app, true, SaveSlot::Two);
	menu_keys(app, &[Key::Enter, Key::ArrowRight, Key::Enter]);
	assert_eq!(
		app.checkpoint_slot(SaveSlot::Two).chapter_label.as_deref(),
		Some("Garden")
	);
	close_checkpoint_menu(app);
}

fn continue_chapter_slots(app: &mut GameApp, recover: bool) {
	open_slot_actions(app, false, SaveSlot::One);
	menu_keys(app, &[Key::Enter]);
	close_checkpoint_menu(app);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
	assert_eq!(progress(app).score, 1);
	open_slot_actions(app, false, SaveSlot::Two);
	if recover {
		menu_keys(app, &[Key::ArrowDown]);
	}
	menu_keys(app, &[Key::Enter]);
	close_checkpoint_menu(app);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "garden");
	assert_eq!(progress(app).score, if recover { 1 } else { 2 });
	if recover {
		move_for(app, Key::A, 90);
		move_for(app, Key::D, 180);
	} else {
		move_for(app, Key::D, 90);
	}
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
	move_for(app, Key::D, 90);
	assert!(progress(app).completed);
	assert_eq!(progress(app).score, 2);
	app.restart_player_game().unwrap();
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "courtyard");
	assert_eq!(progress(app).score, 0);
	assert!(!progress(app).completed);
}

fn close_checkpoint_menu(app: &mut GameApp) {
	for _ in 0..4 {
		if !app.player_controls.is_open() {
			return;
		}
		menu_keys(app, &[Key::Escape]);
	}
	assert!(
		!app.player_controls.is_open(),
		"Escape should return to Play"
	);
}

fn menu_keys(app: &mut GameApp, keys: &[Key]) {
	for key in keys {
		assert!(app.player_controls_key(*key));
	}
}
