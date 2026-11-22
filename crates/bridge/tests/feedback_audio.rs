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

#[test]
fn scaling_preserves_layout_and_signed_samples() {
	let mut bytes = TONE.to_vec();
	let samples = [i16::MIN, -3, 0, 3, i16::MAX];
	for (encoded, sample) in bytes[44..].chunks_exact_mut(2).zip(samples) {
		encoded.copy_from_slice(&sample.to_le_bytes());
	}
	let original = CueClip::from_wave(bytes).unwrap();
	let unchanged = original.scaled(100).unwrap();
	assert!(std::ptr::eq(original.bytes(), unchanged.bytes()));
	let quiet = original.scaled(0).unwrap();
	assert_eq!(&original.bytes()[..44], &quiet.bytes()[..44]);
	assert!(quiet.bytes()[44..].iter().all(|sample| *sample == 0));
	let half = original.scaled(50).unwrap();
	assert_eq!(&original.bytes()[..44], &half.bytes()[..44]);
	let first_samples = half.bytes()[44..]
		.chunks_exact(2)
		.take(samples.len())
		.map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
		.collect::<Vec<_>>();
	assert_eq!(first_samples, [-16_384, -1, 0, 1, 16_383]);
	assert!(original.scaled(101).is_err());
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
