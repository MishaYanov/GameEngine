use super::*;
use runtime::input::{GameInputBinding, MovementBindings};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let directory = std::env::temp_dir().join(format!(
			"game-playtrace-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&directory).unwrap();
		Self(directory)
	}

	fn output(&self) -> PathBuf {
		self.0.join("trace.json")
	}

	fn finish(&self, trace: PlayTrace) -> Vec<Value> {
		trace.finish().unwrap();
		let bytes = fs::read(self.output()).unwrap();
		assert!(bytes.len() <= MAX_TRACE_BYTES);
		let trace: Value = serde_json::from_slice(&bytes).unwrap();
		assert_eq!(trace["schema_version"], 4);
		let actions = trace["actions"].as_array().unwrap();
		assert_eq!(
			actions[0],
			json!({"type":"command","command":{"type":"start_play"}})
		);
		actions[1..].to_owned()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_file(self.output());
		let _ = fs::remove_dir(&self.0);
	}
}

fn profile() -> InputProfile {
	InputProfile {
		actions: vec![GameInputBinding {
			id: "interact".into(),
			key: Key::E,
			animation_parameter: None,
		}],
		..InputProfile::default()
	}
}

#[test]
fn capture_filters_unbound_keys_and_preserves_a_complete_tap_before_tick() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	let mut input = InputState::default();
	input.press(Key::F9);
	trace.record_controls(&input, GamepadSample::default());
	input.press(Key::E);
	trace.record_controls(&input, GamepadSample::default());
	input.release(Key::E);
	trace.record_controls(&input, GamepadSample::default());
	trace.record_steps(0);
	trace.record_steps(2);
	trace.record_steps(3);

	assert_eq!(
		scratch.finish(trace),
		[
			json!({"type":"keyboard_input","keys":["KeyE"]}),
			json!({"type":"keyboard_input","keys":[]}),
			json!({"type":"tick","count":5}),
		]
	);
}

#[test]
fn capture_keeps_the_logical_controller_sample_and_ignores_unauthored_actions() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	let mut input = InputState::default();
	input.press(Key::D);
	trace.record_controls(&input, GamepadSample::default());
	trace.record_controls(
		&input,
		GamepadSample {
			left_x: 100,
			left_y: -100,
			jump: true,
			interact: true,
		},
	);
	trace.clear_controls();
	assert_eq!(
		scratch.finish(trace),
		[
			json!({"type":"keyboard_input","keys":["KeyD"]}),
			json!({"type":"controller_input","left_x":0,"left_y":0,"jump":false,"interact":true}),
			json!({"type":"keyboard_input","keys":[]}),
			json!({"type":"controller_input","left_x":0,"left_y":0,"jump":false,"interact":false}),
			json!({"type":"command","command":{"type":"set_paused","paused":true}}),
			json!({"type":"command","command":{"type":"set_paused","paused":false}}),
		]
	);
}

#[test]
fn a_neutral_focus_boundary_still_clears_a_pending_edge() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	let mut input = InputState::default();
	input.press(Key::E);
	trace.record_controls(&input, GamepadSample::default());
	input.release(Key::E);
	trace.record_controls(&input, GamepadSample::default());
	trace.clear_controls();
	trace.record_steps(1);
	let actions = scratch.finish(trace);
	assert_eq!(actions.len(), 5);
	assert_eq!(actions[2]["command"]["paused"], true);
	assert_eq!(actions[3]["command"]["paused"], false);
	assert_eq!(actions[4], json!({"type":"tick","count":1}));
}

#[test]
fn pause_ignores_unaccepted_control_events_and_ticks() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	trace.record_paused(true);
	let mut input = InputState::default();
	input.press(Key::E);
	trace.record_controls(&input, GamepadSample::default());
	trace.record_steps(1);
	trace.clear_controls();
	trace.record_paused(false);
	trace.record_controls(&input, GamepadSample::default());
	trace.record_steps(1);

	assert_eq!(
		scratch.finish(trace),
		[
			json!({"type":"command","command":{"type":"set_paused","paused":true}}),
			json!({"type":"command","command":{"type":"set_paused","paused":false}}),
			json!({"type":"keyboard_input","keys":["KeyE"]}),
			json!({"type":"tick","count":1}),
		]
	);
}

#[test]
fn a_trace_cannot_exceed_the_headless_tick_budget() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	trace.record_steps(MAX_TRACE_STEPS - 10);
	trace.record_steps(30);
	assert!(trace.is_full());
	trace.clear_controls();
	assert_eq!(
		scratch.finish(trace),
		[json!({"type":"tick","count":MAX_TRACE_STEPS})]
	);
}

#[test]
fn action_and_byte_exhaustion_keep_only_complete_batches() {
	let scratch = Scratch::new();
	let profile = profile();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	for index in 1..MAX_TRACE_ACTIONS - 1 {
		trace.record_paused(index % 2 == 1);
	}
	assert!(!trace.is_full());
	assert!(!trace.paused);
	trace.clear_controls();
	assert!(trace.is_full());
	assert_eq!(trace.actions.len(), MAX_TRACE_ACTIONS - 1);
	let actions = scratch.finish(trace);
	assert_eq!(actions.len(), MAX_TRACE_ACTIONS - 2);
	assert_eq!(actions.last().unwrap()["command"]["paused"], false);

	fs::remove_file(scratch.output()).unwrap();
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	trace.reserved_bytes = MAX_TRACE_BYTES - 20;
	trace.record_paused(true);
	assert!(trace.is_full());
	assert!(scratch.finish(trace).is_empty());
}

#[test]
fn a_snapshot_with_seventeen_held_keys_cannot_escape_the_wire_budget() {
	let scratch = Scratch::new();
	let profile = InputProfile {
		actions: Key::ALL
			.iter()
			.copied()
			.take(17)
			.enumerate()
			.map(|(index, key)| GameInputBinding {
				id: format!("action{index}"),
				key,
				animation_parameter: None,
			})
			.collect(),
		..InputProfile::default()
	};
	let mut trace = PlayTrace::new(scratch.output(), &profile, &profile).unwrap();
	let mut input = InputState::default();
	for action in &profile.actions {
		input.press(action.key);
	}
	trace.record_controls(&input, GamepadSample::default());
	assert!(trace.is_full());
	assert!(scratch.finish(trace).is_empty());
}

#[test]
fn effective_profile_mismatch_and_existing_outputs_are_never_written() {
	let scratch = Scratch::new();
	let authored = profile();
	let different = InputProfile {
		movement: MovementBindings::ARROWS,
		..authored.clone()
	};
	assert!(PlayTrace::new(scratch.output(), &authored, &different).is_err());
	assert!(!scratch.output().exists());

	let trace = PlayTrace::new(scratch.output(), &authored, &authored).unwrap();
	fs::write(scratch.output(), b"an existing file").unwrap();
	assert!(PlayTrace::new(scratch.output(), &authored, &authored).is_err());
	assert!(trace.finish().is_err());
	assert_eq!(fs::read(scratch.output()).unwrap(), b"an existing file");
	assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn alternate_data_streams_are_not_output_files() {
	let scratch = Scratch::new();
	let authored = profile();
	let output = scratch.0.join("trace.json:alternate");
	assert!(PlayTrace::new(output, &authored, &authored).is_err());
	assert!(!scratch.output().exists());
}
