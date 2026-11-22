use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
	fs,
	path::PathBuf,
	process::{Command, Output},
	sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let root = std::env::temp_dir().join(format!(
			"engine-playtrace-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn invoke(&self, version: u32, actions: &[Value], extra_flags: &[&str]) -> Output {
		self.invoke_for(project(), version, actions, extra_flags)
	}

	fn invoke_for(
		&self,
		manifest: PathBuf,
		version: u32,
		actions: &[Value],
		extra_flags: &[&str],
	) -> Output {
		fs::write(
			self.0.join("trace.json"),
			serde_json::to_vec(&json!({ "schema_version": version, "actions": actions })).unwrap(),
		)
		.unwrap();
		Command::new(env!("CARGO_BIN_EXE_engine-headless"))
			.current_dir(&self.0)
			.env("ENGINE_LOG_DIR", self.0.join("logs"))
			.arg("--project")
			.arg(manifest)
			.arg("--replay")
			.arg(self.0.join("trace.json"))
			.arg("--output")
			.arg(self.0.join("report.json"))
			.args(extra_flags)
			.output()
			.unwrap()
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn project() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../projects/cinder-oath/project.json")
		.canonicalize()
		.unwrap()
}

fn start() -> Value {
	json!({ "type": "command", "command": { "type": "start_play" } })
}

fn mark(label: &str) -> Value {
	json!({ "type": "mark", "label": label })
}

fn report(output: Output) -> Value {
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn replay_identity_hashes_exact_input_bytes_and_requires_default_timing() {
	let scratch = Scratch::new();
	let actions = [start()];
	let result = report(scratch.invoke(4, &actions, &[]));
	let replay_hash = format!(
		"{:x}",
		Sha256::digest(fs::read(scratch.0.join("trace.json")).unwrap())
	);
	let project_hash = format!("{:x}", Sha256::digest(fs::read(project()).unwrap()));
	assert_eq!(result["replay_identity"]["replay_sha256"], replay_hash);
	assert_eq!(
		result["replay_identity"]["project_manifest_sha256"],
		project_hash
	);
	assert!(result.get("markers").is_none());

	let altered = report(Scratch::new().invoke(4, &actions, &["--dt", "0.02"]));
	assert!(altered.get("replay_identity").is_none());
	let older = report(Scratch::new().invoke(3, &actions, &[]));
	assert!(older.get("replay_identity").is_none());
}

#[test]
fn malformed_or_out_of_context_marks_fail_before_publishing_report() {
	let over_budget = std::iter::once(start())
		.chain((0..129).map(|index| mark(&format!("mark-{index}"))))
		.collect::<Vec<_>>();
	for (version, actions, expected) in [
		(
			4,
			vec![start(), mark("later")],
			"mark requires replay schema version 5",
		),
		(5, vec![mark("unstarted")], "mark requires Play mode"),
		(5, vec![start(), mark("")], "mark requires a label"),
		(5, vec![start(), mark("a b")], "mark requires a label"),
		(
			5,
			vec![start(), mark("twice"), mark("twice")],
			"Duplicate replay mark",
		),
		(5, over_budget, "Replay exceeds 128 marks"),
	] {
		let scratch = Scratch::new();
		let output = scratch.invoke(version, &actions, &[]);
		assert!(!output.status.success());
		assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
		assert!(!scratch.0.join("report.json").exists());
	}
}
