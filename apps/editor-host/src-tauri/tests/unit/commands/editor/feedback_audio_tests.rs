use super::*;
use bridge::feedback_audio::FeedbackCue;
use std::{
	fs,
	path::PathBuf,
	sync::atomic::{AtomicU64, Ordering},
};

const WAVE: &[u8] =
	include_bytes!("../../../../../../game/fixtures/courtyard-audio/assets/audio/cue-gate.wav");

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Self {
		static NEXT: AtomicU64 = AtomicU64::new(1);
		let root = std::env::temp_dir().join(format!(
			"engine-feedback-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
		));
		let assets = root.join("assets");
		fs::create_dir_all(&assets).unwrap();
		Self(assets)
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(self.0.parent().unwrap());
	}
}

fn assignment(path: &str) -> CueAssignment {
	CueAssignment {
		event: FeedbackCue::GateOpened,
		path: path.into(),
	}
}

#[test]
fn feedback_publication_preserves_catalog_after_rejected_changes() {
	let root = Scratch::new();
	let clip = CueClip::from_wave(WAVE.to_vec()).unwrap();
	let path = save_clip(&root.0, "door", &clip).unwrap();
	assert!(save_clip(&root.0, "door", &clip).is_err());
	assert!(save_clip(&root.0, "../outside", &clip).is_err());
	assert_eq!(
		feedback_audio::load_clip(&root.0, &path).unwrap().bytes(),
		WAVE
	);
	let prepared = apply_catalog(&root.0, None, vec![assignment(&path)]).unwrap();
	let digest = prepared.digest();
	let manifest = root.0.join("audio/cues.json");
	let applied = prepared.manifest();
	assert_eq!(fs::read(&manifest).unwrap(), applied);
	assert!(apply_catalog(&root.0, None, Vec::new()).is_err());
	assert!(apply_catalog(
		&root.0,
		Some(&digest),
		vec![assignment("audio/cue-missing.wav")]
	)
		.is_err());
	assert!(apply_catalog(
		&root.0,
		Some(&digest),
		vec![assignment(&path), assignment(&path)]
	)
		.is_err());
	assert_eq!(fs::read(&manifest).unwrap(), applied);
	let cleared = apply_catalog(&root.0, Some(&digest), Vec::new()).unwrap();
	assert_eq!(fs::read(&manifest).unwrap(), cleared.manifest());
	assert!(cleared.assignments().next().is_none());
	assert!(root.0.join(path).is_file());
}

#[test]
fn feedback_scope_rejects_another_workspace_and_play() {
	let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game/assets");
	let mut workspace =
		EditorWorkspace::open(assets.to_str().unwrap(), "scenes/courtyard.json").unwrap();
	assert!(require_editing(&workspace, Some(workspace.generation)).is_ok());
	assert!(require_editing(&workspace, Some(workspace.generation + 1)).is_err());
	let request = bridge::editor::EditorCommandRequest {
		expected_revision: workspace.backend.session().revision(),
		command: bridge::editor::EditorCommandDto::StartPlay,
	};
	workspace.execute_receipt(request).unwrap();
	assert!(require_editing(&workspace, Some(workspace.generation)).is_err());
}

#[test]
fn feedback_catalog_lock_serializes_processes_and_preserves_current_catalog() {
	let root = Scratch::new();
	let prepared = apply_catalog(&root.0, None, Vec::new()).unwrap();
	let audio = root.0.join("audio");
	let before = fs::read(audio.join("cues.json")).unwrap();
	let held = lock_catalog(&audio).unwrap();
	let result = apply_catalog(&root.0, Some(&prepared.digest()), Vec::new());
	assert!(result.err().unwrap().contains("Another Editor"));
	assert_eq!(fs::read(audio.join("cues.json")).unwrap(), before);
	drop(held);
	assert!(apply_catalog(&root.0, Some(&prepared.digest()), Vec::new()).is_ok());
	assert!(!audio.join("assets.feedback-audio.lock").exists());
	assert!(root.0.with_extension("feedback-audio.lock").is_file());
}
