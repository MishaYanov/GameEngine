//! Exercises the native Player's real polling router with injected samples.

use super::*;
use crate::checkpoint_native_tests::{
	CURRENT_PROJECT_VAR, PHASE_VAR, ProfileDir, move_for, run_isolated_phase, run_native_phase,
};
use courtyard_gameplay::checkpoint::{ContinueAvailability, SaveSlot};
use runtime::input::{FaceButton, MenuButtons, PhysicalGamepadSample};
use std::fs;

const STEP: f32 = 1.0 / 60.0;
const TRACE_NAME: &str = "controller-menu.replay.json";

struct Controller {
	now: Instant,
}

impl Controller {
	fn new() -> Self {
		Self {
			now: Instant::now(),
		}
	}
	fn poll(&mut self, app: &mut GameApp, sample: Option<PhysicalGamepadSample>) {
		self.now = app.sample_standalone_gamepad(self.now, |_| sample).unwrap();
	}
	fn hold(&mut self, app: &mut GameApp, key: Key) {
		let mut sample = PhysicalGamepadSample::default();
		match key {
			Key::Escape => sample.face_buttons[FaceButton::B as usize] = true,
			Key::Enter => sample.face_buttons[FaceButton::A as usize] = true,
			Key::ArrowUp => sample.menu.up = true,
			Key::ArrowDown => sample.menu.down = true,
			Key::ArrowLeft => sample.menu.left = true,
			Key::ArrowRight => sample.menu.right = true,
			_ => panic!("Unsupported menu test input"),
		}
		self.poll(app, Some(sample));
	}
	fn release(&mut self, app: &mut GameApp) {
		self.poll(app, Some(PhysicalGamepadSample::default()));
	}
	fn open(&mut self, app: &mut GameApp) {
		assert!(!app.player_controls.is_open());
		self.release(app);
		self.poll(
			app,
			Some(PhysicalGamepadSample {
				menu: MenuButtons {
					open: true,
					..MenuButtons::default()
				},
				..PhysicalGamepadSample::default()
			}),
		);
		self.release(app);
		assert!(app.player_controls.is_open());
	}
	fn tap(&mut self, app: &mut GameApp, key: Key) {
		self.hold(app, key);
		self.release(app);
	}
	fn advance(&mut self, app: &mut GameApp, count: usize) {
		for _ in 0..count {
			self.tap(app, Key::ArrowDown);
		}
	}
}

#[test]
#[ignore = "requires the release fixture and native Windows Vulkan support"]
fn controller_menus_save_resume_and_isolate_gameplay_across_processes() {
	if let Some(phase) = std::env::var_os(PHASE_VAR) {
		run_native_phase(phase.to_str().unwrap());
		return;
	}
	const TEST: &str = "controller_menu_native_tests::controller_menus_save_resume_and_isolate_gameplay_across_processes";
	let profile = ProfileDir::new();
	let package = std::env::var_os(CURRENT_PROJECT_VAR).map(PathBuf::from);
	run_isolated_phase(TEST, "controller-menu-save", &profile, package.as_deref());
	run_isolated_phase(
		TEST,
		"controller-menu-continue",
		&profile,
		package.as_deref(),
	);
	let trace: serde_json::Value =
		serde_json::from_slice(&fs::read(profile.0.join(TRACE_NAME)).unwrap()).unwrap();
	let controller_events = trace["actions"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|sample| sample["type"] == "controller_input");
	assert!(
		controller_events
			.clone()
			.all(|sample| sample["jump"] == false)
	);
	assert_eq!(
		controller_events
			.filter(|sample| sample["interact"] == true)
			.count(),
		1
	);
}

pub(super) fn run_phase(app: &mut GameApp, phase: &str) {
	let mut controller = Controller::new();
	match phase {
		"controller-menu-save" => {
			remap(app, &mut controller);
			play_and_save(app, &mut controller);
		}
		"controller-menu-continue" => {
			assert_eq!(app.player_controls.controller().interact, FaceButton::B);
			assert_eq!(
				app.checkpoint_slot(SaveSlot::One).primary,
				ContinueAvailability::Empty
			);
			assert_eq!(
				app.checkpoint_slot(SaveSlot::Two).primary,
				ContinueAvailability::Current
			);
			let run = app.editor.play_status().unwrap().run_id;
			controller.open(app);
			controller.advance(app, 3);
			controller.tap(app, Key::Enter);
			controller.tap(app, Key::ArrowDown);
			controller.tap(app, Key::Enter);
			controller.tap(app, Key::Enter);
			assert_ne!(app.editor.play_status().unwrap().run_id, run);
			assert!(gate_is_open(app));
		}
		_ => panic!("Unknown controller menu phase"),
	}
}

fn remap(app: &mut GameApp, controller: &mut Controller) {
	controller.open(app);
	controller.advance(app, 4);
	controller.tap(app, Key::Enter);
	for key in [
		Key::ArrowDown,
		Key::ArrowLeft,
		Key::ArrowDown,
		Key::Enter,
		Key::Escape,
	] {
		controller.tap(app, key);
	}
	assert!(!app.player_controls.is_open());
	assert_eq!(app.player_controls.controller().interact, FaceButton::B);
}

fn play_and_save(app: &mut GameApp, controller: &mut Controller) {
	let profile = app.effective_input_profile().cloned().unwrap();
	let output = PathBuf::from(std::env::var_os("GAME_ENGINE_QA_CHECKPOINT_PROFILE").unwrap())
		.join(TRACE_NAME);
	app.play_trace = Some(playtrace::PlayTrace::new(output, &profile, &profile).unwrap());
	move_for(app, Key::D, 90);
	move_for(app, Key::A, 84);
	assert!(!gate_is_open(app));
	controller.open(app);
	controller.hold(app, Key::Escape);
	assert!(!app.player_controls.is_open());
	controller.hold(app, Key::Escape);
	app.tick_play(STEP).unwrap();
	assert!(!gate_is_open(app));
	controller.release(app);
	controller.tap(app, Key::Escape);
	app.tick_play(STEP).unwrap();
	assert!(gate_is_open(app));
	move_for(app, Key::A, 64);
	controller.open(app);
	controller.advance(app, 2);
	controller.tap(app, Key::Enter);
	controller.tap(app, Key::ArrowDown);
	controller.tap(app, Key::Enter);
	controller.tap(app, Key::Enter);
	assert!(menu_text(app).contains("Saved after_gate"));
	let saved = app.checkpoint_slot(SaveSlot::Two).generation.unwrap();
	controller.tap(app, Key::Enter);
	assert!(menu_text(app).contains("> Cancel"));
	controller.poll(app, None);
	assert!(app.player_controls_key(Key::Escape));
	assert!(!menu_text(app).contains("> Cancel"));
	assert!(app.player_controls_key(Key::Enter));
	controller.hold(app, Key::Enter);
	assert!(menu_text(app).contains("> Cancel"));
	controller.release(app);
	controller.tap(app, Key::Enter);
	assert!(!menu_text(app).contains("> Cancel"));
	assert_eq!(
		app.checkpoint_slot(SaveSlot::Two).generation,
		Some(saved),
		"cancel left both generations untouched",
	);
	controller.tap(app, Key::Enter);
	assert!(menu_text(app).contains("> Cancel"));
	controller.tap(app, Key::ArrowDown);
	assert!(menu_text(app).contains("> Replace save"));
	app.save_checkpoint_slot(SaveSlot::Two, &saved).unwrap();
	let intervening = app.checkpoint_slot(SaveSlot::Two).generation.unwrap();
	assert_ne!(intervening, saved);
	controller.tap(app, Key::Enter);
	assert!(menu_text(app).contains("save slot changed"));
	assert_eq!(
		app.checkpoint_slot(SaveSlot::Two).generation,
		Some(intervening)
	);
	controller.tap(app, Key::Enter);
	controller.tap(app, Key::ArrowDown);
	controller.tap(app, Key::Enter);
	assert!(menu_text(app).contains("Saved after_gate"));
	assert_eq!(
		app.checkpoint_slot(SaveSlot::Two).primary,
		ContinueAvailability::Current
	);
	app.play_trace.take().unwrap().finish().unwrap();
}

fn menu_text(app: &GameApp) -> String {
	app.player_controls.rows().unwrap().join("\n")
}

fn gate_is_open(app: &GameApp) -> bool {
	app.editor
		.play_progress::<progress::CampaignProgress>()
		.unwrap()
		.route_gate_open
}
