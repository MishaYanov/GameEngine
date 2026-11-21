use std::{
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};

use bridge::feedback_audio::{CueClip, FeedbackCue, load};
use serde_json::{Value, json};

const TONE: &[u8] =
	include_bytes!("../../../apps/game/fixtures/courtyard-audio/assets/audio/cue-key.wav");

struct Folder(PathBuf);

impl Folder {
	fn new() -> Self {
		static NEXT: AtomicUsize = AtomicUsize::new(0);
		let root = std::env::temp_dir().join(format!(
			"cue-test-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		fs::create_dir(&root).unwrap();
		Self(root)
	}

	fn catalog(&self, cues: Value) {
		let audio = self.0.join("audio");
		fs::create_dir_all(&audio).unwrap();
		fs::write(
			audio.join("cues.json"),
			json!({"schema_version": 1, "cues": cues}).to_string(),
		)
			.unwrap();
	}

	fn path(&self) -> &Path {
		&self.0
	}
}

impl Drop for Folder {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

#[test]
fn absence_empty_and_shared_clips_are_distinct() {
	let folder = Folder::new();
	assert!(load(folder.path()).unwrap().is_none());
	folder.catalog(json!([]));
	let empty = load(folder.path()).unwrap().unwrap();
	assert_eq!(empty.files().count(), 1);
	assert!(empty.clip(FeedbackCue::GateOpened).is_none());

	folder.catalog(json!([
		{"event": "key_collected", "path": "audio/cue-key.wav"},
		{"event": "gate_opened", "path": "audio/cue-key.wav"}
	]));
	assert!(load(folder.path()).is_err());
	fs::write(folder.path().join("audio/cue-key.wav"), TONE).unwrap();
	let valid = load(folder.path()).unwrap().unwrap();
	assert_eq!(valid.files().count(), 2);
	assert_eq!(valid.clip(FeedbackCue::GateOpened).unwrap().bytes(), TONE);
}

#[test]
fn rejects_duplicate_unknown_and_unconfined_entries() {
	let folder = Folder::new();
	for cues in [
		json!([
			{"event": "gate_opened", "path": "audio/cue-key.wav"},
			{"event": "gate_opened", "path": "audio/cue-key.wav"}
		]),
		json!([{"event": "jumped", "path": "audio/cue-key.wav"}]),
		json!([{"event": "gate_opened", "path": "audio/../cue-key.wav"}]),
	] {
		folder.catalog(cues);
		fs::write(folder.path().join("audio/cue-key.wav"), TONE).unwrap();
		assert!(load(folder.path()).is_err());
	}
}

#[test]
fn rejects_invalid_pcm_before_it_can_reach_playback() {
	for bytes in [vec![], TONE[..TONE.len() - 1].to_vec(), vec![0; 16_045], {
		let mut bytes = TONE.to_vec();
		bytes[24] = 0;
		bytes
	}] {
		assert!(CueClip::from_wave(bytes).is_err());
	}
	assert!(CueClip::from_wave(TONE.to_vec()).is_ok());
}

#[cfg(unix)]
#[test]
fn linked_audio_directory_is_rejected_even_without_a_catalog() {
	use std::os::unix::fs::symlink;
	let folder = Folder::new();
	let outside = Folder::new();
	symlink(outside.path(), folder.path().join("audio")).unwrap();
	assert!(load(folder.path()).is_err());
}
