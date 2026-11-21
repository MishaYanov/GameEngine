use serde_json::{Value, json};
use std::{
	fs,
	path::PathBuf,
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};
struct Scratch(PathBuf);
impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../target")
			.join(format!(
				"qa-animation-replay-{}-{}",
				std::process::id(),
				NEXT.fetch_add(1, Ordering::Relaxed)
			));
		fs::create_dir(&path).unwrap();
		Self(path)
	}
}
impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}
fn run(path: &std::path::Path, actions: &[Value]) -> Value {
	fs::write(
		path,
		serde_json::to_vec(&json!({"schema_version":1,"actions":actions})).unwrap(),
	)
	.unwrap();
	let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../game/animation-demo.json");
	let output = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--project")
		.arg(project)
		.arg("--replay")
		.arg(path)
		.output()
		.unwrap();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}
fn pose<'a>(summary: &'a Value, key: &str) -> &'a Value {
	summary["native_components"]["animations"]
		.as_array()
		.unwrap()
		.iter()
		.find(|pose| pose["key"] == key)
		.unwrap()
}
fn start() -> Value {
	json!({"type":"command","command":{"type":"start_play"}})
}

#[test]
fn animation_replay_hash_includes_pose_clock_palette_and_bounds_and_restart_resets_them() {
	let temp = Scratch::new();
	let file = temp.0.join("replay.json");
	let mut actions = vec![
		start(),
		json!({"type":"tick","count":1}),
		json!({"type":"command","command":{"type":"set_character_input","key":"player","movement_xz":[1,0],"jump_held":true}}),
		json!({"type":"command","command":{"type":"set_character_input","key":"player","movement_xz":[1,0],"jump_held":false}}),
		json!({"type":"tick","count":9}),
	];
	let a = run(&file, &actions);
	let b = run(&file, &actions);
	assert_eq!(a["native_components_hash_scope"], "native_components_v1");
	assert_eq!(a["native_components_hash"].as_str().unwrap().len(), 16);
	assert_eq!(a["native_components_hash"], b["native_components_hash"]);
	assert_eq!(a["native_components"], b["native_components"]);
	assert_eq!(pose(&a, "player")["mode"], "air");
	assert_eq!(pose(&a, "player")["clip_index"], 2);
	assert!(pose(&a, "player")["elapsed_ticks"].as_u64().unwrap() > 0);
	assert_eq!(pose(&a, "clip-demo")["elapsed_ticks"], 10);
	assert_eq!(pose(&a, "player")["palette"].as_array().unwrap().len(), 2);
	assert_eq!(
		pose(&a, "player")["bounds"]["min"]
			.as_array()
			.unwrap()
			.len(),
		3
	);
	actions.push(json!({"type":"restart_run"}));
	let restarted = run(&file, &actions);
	let fresh = run(&file, &[start()]);
	assert_eq!(restarted["native_components"], fresh["native_components"]);
	assert_eq!(
		restarted["native_components_hash"],
		fresh["native_components_hash"]
	);
	assert_ne!(
		a["native_components_hash"],
		restarted["native_components_hash"]
	);
	assert_eq!(pose(&restarted, "player")["elapsed_ticks"], 0);
	assert_eq!(pose(&restarted, "clip-demo")["elapsed_ticks"], 0);
	assert!(pose(&restarted, "player")["fade"].is_null());
}

#[test]
fn animation_native_projection_is_independent_of_catchup_partition_and_pause() {
	let temp = Scratch::new();
	let file = temp.0.join("replay.json");
	let split = run(&file, &[start(), json!({"type":"tick","count":60})]);
	let combined = run(
		&file,
		&[
			start(),
			json!({"type":"tick","count":15,"elapsed_seconds":4./60.}),
		],
	);
	assert_eq!(split["fixed_steps"], 60);
	assert_eq!(combined["fixed_steps"], 60);
	assert_eq!(split["native_components"], combined["native_components"]);
	assert_eq!(
		split["native_components_hash"],
		combined["native_components_hash"]
	);
	let paused = run(
		&file,
		&[
			start(),
			json!({"type":"tick","count":60}),
			json!({"type":"command","command":{"type":"set_paused","paused":true}}),
			json!({"type":"tick","count":100}),
		],
	);
	assert_eq!(paused["native_components"], split["native_components"]);
	assert_eq!(
		paused["native_components_hash"],
		split["native_components_hash"]
	);
}

#[test]
fn edit_mode_reports_null_native_state_and_help_describes_cpu_animation_decoding() {
	let temp = Scratch::new();
	let edit = run(&temp.0.join("edit.json"), &[]);
	assert!(edit["native_components"].is_null());
	assert_eq!(edit["native_components_hash_bytes"], 4);
	assert_eq!(edit["state_hash_scope"], "project_and_render_scene");
	let help = Command::new(env!("CARGO_BIN_EXE_engine-headless"))
		.arg("--help")
		.output()
		.unwrap();
	assert!(help.status.success());
	let text = String::from_utf8(help.stdout).unwrap();
	assert!(text.contains("Animated assets are decoded on the CPU"));
	assert!(text.contains("No GPU or window"));
	assert!(text.contains("opaque"));
}
