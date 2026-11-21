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

	if let Some(headless) = std::env::var_os(HEADLESS_VAR) {
		let manifest = package
			.unwrap_or_else(|| {
				PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("project.release-learning.json")
			})
			.canonicalize()
			.unwrap();
		let output = Command::new(PathBuf::from(headless).canonicalize().unwrap())
			.current_dir(&profile.0)
			.arg("--project")
			.arg(manifest)
			.arg("--replay")
			.arg(&trace)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		let report: Value = serde_json::from_slice(&output.stdout).unwrap();
		assert_eq!(report["play_scene_id"], "finish");
		assert_eq!(report["play_status"]["gameplay"]["completed"], true);
		assert_eq!(report["play_status"]["gameplay"]["score"], 1);
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
		"Press B to open the route gate",
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
