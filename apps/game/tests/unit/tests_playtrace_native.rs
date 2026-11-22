//! Windows Player remapping and admitted input replay across isolated processes.
use super::*;
use crate::checkpoint_native_tests::{
	CURRENT_PROJECT_VAR, PHASE_VAR, ProfileDir, run_isolated_phase, run_native_phase,
};
use runtime::input::{FaceButton, PhysicalGamepadSample};
use serde_json::{Value, json};
use std::{fs, process::Command};

const TRACE_NAME: &str = "courtyard-captured.replay.json";
const HEADLESS_VAR: &str = "GAME_ENGINE_QA_HEADLESS";
const STEP: f32 = 1.0 / 60.0;

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn native_controller_remap_persists_and_playtrace_replays() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"playtrace_native_tests::native_controller_remap_persists_and_playtrace_replays";
	let profile = ProfileDir::new();
	let package = std::env::var_os(CURRENT_PROJECT_VAR).map(PathBuf::from);
	run_isolated_phase(TEST, "trace-remap", &profile, package.as_deref());
	run_isolated_phase(TEST, "trace-record", &profile, package.as_deref());
	let trace = profile.0.join(TRACE_NAME);
	let payload: Value = serde_json::from_slice(&fs::read(&trace).unwrap()).unwrap();
	assert_eq!(payload["schema_version"], 4);
	let actions = payload["actions"].as_array().unwrap();
	assert_eq!(
		actions[0],
		json!({"type": "command", "command": {"type": "start_play"}})
	);
	assert!(
		actions
			.iter()
			.any(|action| action == &json!({"type": "keyboard_input", "keys": ["KeyD"]}))
	);
	assert!(
		actions
			.iter()
			.any(|action| action == &json!({"type": "keyboard_input", "keys": ["KeyA"]}))
	);
	assert_eq!(
		actions
			.iter()
			.filter(|action| action["type"] == "controller_input" && action["interact"] == true)
			.count(),
		1,
		"the remapped B tap should emit exactly one logical press",
	);
	assert!(actions.windows(2).any(|window| {
		window[0]["type"] == "controller_input"
			&& window[0]["interact"] == true
			&& window[1]["type"] == "controller_input"
			&& window[1]["interact"] == false
	}));
	assert!(
		!String::from_utf8(fs::read(&trace).unwrap())
			.unwrap()
			.contains("F1")
	);

	if let Some(report) = replay_in_headless(package.as_deref(), &profile.0, &trace) {
		assert_completed_courtyard(&report);
		println!("NATIVE_PLAYTRACE_REPLAY_OK");
	}
	if let Some(output) = std::env::var_os("GAME_ENGINE_QA_PLAYTRACE_OUTPUT") {
		runtime::scene_io::save_new_bytes(&PathBuf::from(output), &fs::read(trace).unwrap())
			.unwrap();
	}
}

pub(super) fn run_phase(app: &mut GameApp, phase: &str) {
	match phase {
		"trace-remap" => remap(app),
		"trace-record" => record(app),
		phase if phase.starts_with("trace-keyboard-") => native_keyboard_profile_phase(app, phase),
		phase if phase.starts_with("preset-library-") => native_library_phase(app, phase),
		phase if phase.starts_with("preset-") => native_preset_phase(app, phase),
		_ => panic!("Unknown player trace phase"),
	}
}

fn remap(app: &mut GameApp) {
	assert_eq!(app.player_controls.controller().interact, FaceButton::X);
	for key in [
		Key::Escape,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::ArrowDown,
		Key::Enter,
		Key::ArrowDown,
		Key::ArrowLeft,
		Key::ArrowDown,
		Key::Enter,
		Key::Escape,
	] {
		assert!(app.player_controls_key(key));
	}
	assert!(!app.player_controls.is_open());
	assert_eq!(app.player_controls.controller().interact, FaceButton::B);
}

fn record(app: &mut GameApp) {
	assert_eq!(app.player_controls.controller().interact, FaceButton::B);
	let profile = app
		.project
		.as_ref()
		.unwrap()
		.input_profile
		.clone()
		.unwrap_or_default();
	let output = PathBuf::from(std::env::var_os("GAME_ENGINE_QA_CHECKPOINT_PROFILE").unwrap())
		.join(TRACE_NAME);
	app.play_trace = Some(
		playtrace::PlayTrace::new(
			output,
			&profile,
			app.effective_input_profile().unwrap_or(&profile),
		)
		.unwrap(),
	);
	app.interaction_action = Some("interact".into());
	app.input.press(Key::F1);
	app.submit_play_controls().unwrap();
	app.input.release(Key::F1);
	app.submit_play_controls().unwrap();
	move_for(app, Key::D, 90);
	assert!(progress(app).has_courtyard_key);
	move_for(app, Key::A, 84);
	let mut now = Instant::now();
	submit_pad(app, &mut now, None);
	submit_pad(app, &mut now, Some(FaceButton::X));
	submit_pad(app, &mut now, None);
	app.tick_play(STEP).unwrap();
	assert!(!progress(app).route_gate_open);
	submit_pad(app, &mut now, Some(FaceButton::B));
	assert_eq!(
		app.player_controls.prompt_device(),
		PromptDevice::Controller
	);
	assert_eq!(
		scene_rendering::interact_hint(
			app.effective_input_profile(),
			app.interaction_action.as_deref(),
			app.player_controls.controller(),
			app.player_controls.prompt_device(),
		),
		"Press B (Controller) to open the route gate",
	);
	submit_pad(app, &mut now, None);
	app.tick_play(STEP).unwrap();
	assert!(progress(app).route_gate_open);
	move_for(app, Key::A, 64);
	move_for(app, Key::A, 30);
	assert_eq!(app.editor.play_scene_id().unwrap().as_str(), "finish");
	move_for(app, Key::D, 90);
	assert!(progress(app).completed);
	assert_eq!(progress(app).score, 1);
	app.play_trace.take().unwrap().finish().unwrap();
}

fn move_for(app: &mut GameApp, key: Key, ticks: u32) {
	app.input.press(key);
	app.submit_play_controls().unwrap();
	for _ in 0..ticks {
		app.tick_play(STEP).unwrap();
		app.input.end_frame();
	}
	app.input.release(key);
	app.submit_play_controls().unwrap();
}

fn submit_pad(app: &mut GameApp, now: &mut Instant, face: Option<FaceButton>) {
	let previous = app.logical_gamepad();
	let mut sample = PhysicalGamepadSample::default();
	if let Some(face) = face {
		sample.face_buttons[face as usize] = true;
	}
	if app.play_gamepad.poll(*now, |_| Some(sample)) {
		app.submit_play_controls().unwrap();
		app.note_controller_activity(previous);
	}
	*now += Duration::from_millis(17);
}

fn progress(app: &GameApp) -> &progress::CampaignProgress {
	app.editor.play_progress().unwrap()
}

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn native_keyboard_remap_restores_authored_playtrace_across_processes() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str = "playtrace_native_tests::native_keyboard_remap_restores_authored_playtrace_across_processes";
	let isolated = ProfileDir::new();
	let package = std::env::var_os(CURRENT_PROJECT_VAR).map(PathBuf::from);
	for phase in [
		"trace-keyboard-remap",
		"trace-keyboard-verify",
		"trace-keyboard-record",
	] {
		run_isolated_phase(TEST, phase, &isolated, package.as_deref());
	}
	let trace = isolated.0.join(TRACE_NAME);
	let payload: Value = serde_json::from_slice(&fs::read(&trace).unwrap()).unwrap();
	let actions = payload["actions"].as_array().unwrap();
	assert!(
		actions
			.iter()
			.any(|action| action == &json!({"type": "keyboard_input", "keys": ["KeyE"]}))
	);
	assert!(
		!actions
			.iter()
			.any(|action| action == &json!({"type": "keyboard_input", "keys": ["KeyF"]}))
	);
	if let Some(report) = replay_in_headless(package.as_deref(), &isolated.0, &trace) {
		assert_completed_courtyard(&report);
		println!("NATIVE_KEYBOARD_PLAYTRACE_REPLAY_OK");
	}
}

fn replay_in_headless(package: Option<&Path>, profile_root: &Path, trace: &Path) -> Option<Value> {
	let executable = PathBuf::from(std::env::var_os(HEADLESS_VAR)?);
	let manifest = package.map_or_else(
		|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.release-learning.json"),
		Path::to_path_buf,
	);
	let output = Command::new(executable.canonicalize().unwrap())
		.current_dir(profile_root)
		.arg("--project")
		.arg(manifest.canonicalize().unwrap())
		.arg("--replay")
		.arg(trace)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	Some(serde_json::from_slice(&output.stdout).unwrap())
}

fn assert_completed_courtyard(report: &Value) {
	assert_eq!(report["play_scene_id"], "finish");
	assert_eq!(report["play_status"]["gameplay"]["completed"], true);
	assert_eq!(report["play_status"]["gameplay"]["score"], 1);
}

fn open_keyboard_menu(app: &mut GameApp) {
	if !app.player_controls.is_open() {
		assert!(app.player_controls_key(Key::Escape));
	}
	for _ in 0..12 {
		if app.player_controls.rows_with_capacity(1).unwrap() == ["> Keyboard"] {
			assert!(app.player_controls_key(Key::Enter));
			return;
		}
		assert!(app.player_controls_key(Key::ArrowDown));
	}
	panic!("Keyboard Controls page unavailable");
}

fn tap(app: &mut GameApp, key: Key) {
	app.input.press(key);
	app.submit_play_controls().unwrap();
	app.input.release(key);
	app.submit_play_controls().unwrap();
	app.tick_play(STEP).unwrap();
}

fn native_keyboard_profile_phase(app: &mut GameApp, phase: &str) {
	let authored = app.project.as_ref().unwrap().input_profile.clone().unwrap();
	let output = PathBuf::from(std::env::var_os("GAME_ENGINE_QA_CHECKPOINT_PROFILE").unwrap())
		.join(TRACE_NAME);
	match phase {
		"trace-keyboard-remap" => {
			open_keyboard_menu(app);
			for key in [Key::Enter, Key::F, Key::ArrowDown, Key::Enter, Key::Escape] {
				assert!(app.player_controls_key(key));
			}
			assert_eq!(
				app.effective_input_profile().unwrap().actions[0].key,
				Key::F
			);
		}
		"trace-keyboard-verify" => {
			let effective = app.effective_input_profile().unwrap();
			assert_eq!(effective.actions[0].key, Key::F);
			assert!(
				playtrace::PlayTrace::new(output, &authored, effective)
					.err()
					.unwrap()
					.contains("Restore Keyboard defaults")
			);
			move_for(app, Key::D, 90);
			move_for(app, Key::A, 84);
			tap(app, Key::E);
			assert!(!progress(app).route_gate_open);
			tap(app, Key::F);
			assert!(progress(app).route_gate_open);
			open_keyboard_menu(app);
			for key in [
				Key::ArrowDown,
				Key::ArrowDown,
				Key::Enter,
				Key::ArrowUp,
				Key::Enter,
				Key::Escape,
			] {
				assert!(app.player_controls_key(key));
			}
			assert_eq!(app.effective_input_profile().unwrap(), &authored);
		}
		"trace-keyboard-record" => {
			let effective = app.effective_input_profile().unwrap();
			app.play_trace = Some(playtrace::PlayTrace::new(output, &authored, effective).unwrap());
			open_keyboard_menu(app);
			for key in [Key::Enter, Key::F, Key::ArrowDown, Key::Enter] {
				assert!(app.player_controls_key(key));
			}
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.last()
					.unwrap()
					.contains("recording")
			);
			for _ in 0..2 {
				assert!(app.player_controls_key(Key::Escape));
			}
			assert_eq!(app.effective_input_profile().unwrap(), &authored);
			move_for(app, Key::D, 90);
			move_for(app, Key::A, 84);
			tap(app, Key::E);
			assert!(progress(app).route_gate_open);
			move_for(app, Key::A, 64);
			move_for(app, Key::A, 30);
			move_for(app, Key::D, 90);
			assert!(progress(app).completed);
			app.play_trace.take().unwrap().finish().unwrap();
		}
		_ => panic!("Unknown keyboard replay phase"),
	}
}

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn native_input_preset_transfers_across_player_profiles() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"playtrace_native_tests::native_input_preset_transfers_across_player_profiles";
	let rehearsal = ProfileDir::new();
	let source = ProfileDir(rehearsal.0.join("source"));
	let destination = ProfileDir(rehearsal.0.join("destination"));
	let transfers = rehearsal.0.join("presets");
	for directory in [&source.0, &destination.0, &transfers] {
		fs::create_dir(directory).unwrap();
	}
	run_isolated_phase(TEST, "preset-export", &source, None);
	let preset = fs::read_dir(&transfers)
		.unwrap()
		.next()
		.unwrap()
		.unwrap()
		.path();
	let exported = fs::read(&preset).unwrap();
	run_isolated_phase(TEST, "preset-apply", &destination, None);
	run_isolated_phase(TEST, "preset-relaunch", &destination, None);
	assert_eq!(fs::read(preset).unwrap(), exported);
}

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn native_named_layout_library_round_trips() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str = "playtrace_native_tests::native_named_layout_library_round_trips";
	let profile = ProfileDir::new();
	for phase in [
		"preset-library-save",
		"preset-library-review",
		"preset-library-relaunch",
	] {
		run_isolated_phase(TEST, phase, &profile, None);
	}
}

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn native_layout_library_backup_restores_without_applying_controls() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str =
		"playtrace_native_tests::native_layout_library_backup_restores_without_applying_controls";
	let rehearsal = ProfileDir::new();
	let source = ProfileDir(rehearsal.0.join("source"));
	let destination = ProfileDir(rehearsal.0.join("destination"));
	let backups = rehearsal.0.join("presets");
	for path in [&source.0, &destination.0, &backups] {
		fs::create_dir(path).unwrap();
	}
	run_isolated_phase(TEST, "preset-library-save", &source, None);
	run_isolated_phase(TEST, "preset-library-backup-export", &source, None);
	let backup = fs::read_dir(&backups)
		.unwrap()
		.next()
		.unwrap()
		.unwrap()
		.path();
	assert!(backup.to_string_lossy().ends_with(".input-library.json"));
	let bytes = fs::read(&backup).unwrap();
	run_isolated_phase(TEST, "preset-library-backup-restore", &destination, None);
	run_isolated_phase(TEST, "preset-library-backup-relaunch", &destination, None);
	assert_eq!(fs::read(backup).unwrap(), bytes);
}

fn native_library_phase(app: &mut GameApp, phase: &str) {
	let authored = app.project.as_ref().unwrap().input_profile.clone().unwrap();
	let effective =
		|app: &GameApp| app.effective_input_profile().unwrap_or(&authored).actions[0].key;
	if phase.starts_with("preset-library-backup-") {
		let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
		app.transfer_directory = Some(profile.parent().unwrap().join("presets"));
		select_preset_row(app, "> Input presets");
		select_preset_row(app, "> Saved Layouts");
	}
	match phase {
		"preset-library-backup-export" => {
			select_preset_row(app, "> Export Library");
			let rows = app.player_controls.rows().unwrap().join("\n");
			for text in ["LAYOUT 1", "LAYOUT 2", "Exported 2 layouts"] {
				assert!(rows.contains(text), "{rows}");
			}
		}
		"preset-library-backup-restore" => {
			assert_eq!(effective(app), Key::E);
			let file = fs::read_dir(app.transfer_directory.as_ref().unwrap())
				.unwrap()
				.next()
				.unwrap()
				.unwrap();
			let name = file.file_name();
			let choice = format!(
				"> {}",
				crate::local_player_files::transfer_label(
					"Backup",
					1,
					name.to_str().unwrap(),
					".input-library.json",
				)
			);
			select_preset_row(app, "> Review Backup");
			select_preset_row(app, &choice);
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Keep Current"]
			);
			select_preset_row(app, "> Keep Current");
			assert!(app.player_controls_key(Key::Escape));
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.iter()
					.any(|row| row.contains("No saved layouts"))
			);
			select_preset_row(app, "> Review Backup");
			select_preset_row(app, &choice);
			select_preset_row(app, "> Restore Layouts");
			assert_eq!(effective(app), Key::E);
			select_preset_row(app, "> LAYOUT 2");
			select_preset_row(app, "> Review layout");
			select_preset_row(app, "> Apply preset");
			assert_eq!(effective(app), Key::F);
		}
		"preset-library-backup-relaunch" => {
			assert_eq!(effective(app), Key::F);
			let rows = app.player_controls.rows().unwrap().join("\n");
			for text in ["LAYOUT 1", "LAYOUT 2"] {
				assert!(rows.contains(text), "{rows}");
			}
		}
		"preset-library-save" => {
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Save Current Layout");
			select_preset_row(app, "> Save LAYOUT 1");
			for _ in 0..3 {
				assert!(app.player_controls_key(Key::Escape));
			}
			native_keyboard_profile_phase(app, "trace-keyboard-remap");
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Save Current Layout");
			select_preset_row(app, "> Save LAYOUT 2");
			assert_eq!(effective(app), Key::F);
		}
		"preset-library-review" => {
			assert_eq!(effective(app), Key::F);
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Saved Layouts");
			select_preset_row(app, "> LAYOUT 1");
			select_preset_row(app, "> Review layout");
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Keep current"]
			);
			select_preset_row(app, "> Keep current");
			assert_eq!(effective(app), Key::F);
			select_preset_row(app, "> Rename layout");
			select_preset_row(app, "> Erase last character");
			select_preset_row(app, "> Add A");
			select_preset_row(app, "> Save LAYOUT A");
			select_preset_row(app, "> LAYOUT 2");
			select_preset_row(app, "> Delete layout");
			select_preset_row(app, "> Keep layout");
			select_preset_row(app, "> Delete layout");
			select_preset_row(app, "> Delete layout");
			select_preset_row(app, "> LAYOUT A");
			select_preset_row(app, "> Review layout");
			select_preset_row(app, "> Apply preset");
			assert_eq!(effective(app), Key::E);
		}
		"preset-library-relaunch" => {
			assert_eq!(effective(app), Key::E);
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Saved Layouts");
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.iter()
					.any(|row| row == "> LAYOUT A")
			);
			assert!(
				!app.player_controls
					.rows()
					.unwrap()
					.iter()
					.any(|row| row.contains("LAYOUT 2"))
			);
			for _ in 0..3 {
				assert!(app.player_controls_key(Key::Escape));
			}
			move_for(app, Key::D, 90);
			move_for(app, Key::A, 84);
			tap(app, Key::F);
			assert!(!progress(app).route_gate_open);
			tap(app, Key::E);
			assert!(progress(app).route_gate_open);
		}
		_ => panic!("Unknown named layout phase"),
	}
}

fn native_preset_phase(app: &mut GameApp, phase: &str) {
	let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
	app.transfer_directory = Some(profile.parent().unwrap().join("presets"));
	match phase {
		"preset-export" => {
			native_keyboard_profile_phase(app, "trace-keyboard-remap");
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Export local preset");
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.last()
					.unwrap()
					.contains("Exported input")
			);
		}
		"preset-apply" => {
			let authored = app.project.as_ref().unwrap().input_profile.clone().unwrap();
			assert_eq!(
				app.effective_input_profile().unwrap_or(&authored).actions[0].key,
				Key::E
			);

			// The review defaults to a non-writing action even in the shortest layout.
			select_preset_row(app, "> Input presets");
			select_preset_row(app, "> Review preset file");
			assert!(
				app.player_controls.rows_with_capacity(1).unwrap()[0].starts_with("> Preset 01:")
			);
			assert!(app.player_controls_key(Key::Enter));
			assert_eq!(
				app.player_controls.rows_with_capacity(1).unwrap(),
				["> Keep current"]
			);
			assert!(app.player_controls_key(Key::Enter));
			assert!(app.effective_input_profile().unwrap_or(&authored).actions[0].key == Key::E);

			app.play_trace = Some(
				playtrace::PlayTrace::new(
					profile.join("blocked.replay.json"),
					&authored,
					&authored,
				)
				.unwrap(),
			);
			assert!(app.player_controls_key(Key::Enter));
			select_preset_row(app, "> Apply preset");
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.last()
					.unwrap()
					.contains("recording")
			);
			assert_eq!(
				app.effective_input_profile().unwrap_or(&authored).actions[0].key,
				Key::E
			);
			app.play_trace.take();

			select_preset_row(app, "> Apply preset");
			assert_eq!(
				app.effective_input_profile().unwrap().actions[0].key,
				Key::F
			);
			assert!(
				app.player_controls
					.rows()
					.unwrap()
					.last()
					.unwrap()
					.contains("Input preset applied")
			);
		}
		"preset-relaunch" => {
			assert_eq!(
				app.effective_input_profile().unwrap().actions[0].key,
				Key::F
			);
			move_for(app, Key::D, 90);
			move_for(app, Key::A, 84);
			tap(app, Key::E);
			assert!(!progress(app).route_gate_open);
			tap(app, Key::F);
			assert!(progress(app).route_gate_open);
		}
		_ => panic!("Unknown input preset phase"),
	}
}

fn select_preset_row(app: &mut GameApp, label: &str) {
	if !app.player_controls.is_open() {
		assert!(app.player_controls_key(Key::Escape));
	}
	for _ in 0..30 {
		if app.player_controls.rows_with_capacity(1).unwrap() == [label] {
			assert!(app.player_controls_key(Key::Enter));
			return;
		}
		assert!(app.player_controls_key(Key::ArrowDown));
	}
	panic!("Preset row {label} unavailable");
}
