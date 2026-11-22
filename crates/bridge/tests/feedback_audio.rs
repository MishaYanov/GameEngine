use std::{
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};

use bridge::feedback_audio::{CueClip, FeedbackBus, FeedbackCue, combined_percent, load};
use engine_core::scene_catalog::{SceneCatalog, SceneDescriptor, SceneId};
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

#[test]
fn reads_both_catalog_versions_and_validates_registered_scene_mix() {
	let folder = Folder::new();
	fs::create_dir(folder.path().join("audio")).unwrap();
	let manifest = folder.path().join("audio/cues.json");
	let v1 = br#"{ "schema_version": 1, "cues": [] }"#;
	fs::write(&manifest, v1).unwrap();
	let legacy = load(folder.path()).unwrap().unwrap();
	assert_eq!(legacy.manifest(), v1.as_slice());
	let modern_without_levels = br#"{ "schema_version": 2, "cues": [] }"#;
	fs::write(&manifest, modern_without_levels).unwrap();
	let defaults = load(folder.path()).unwrap().unwrap();
	assert_eq!(defaults.mix(), legacy.mix());

	let authored = br#"{ "schema_version": 2, "cues": [],
		"bus_percent": {"mechanisms": 80},
		"scene_bus_percent": {"garden": {"mechanisms": 50}} }"#;
	fs::write(&manifest, authored).unwrap();
	let catalog = load(folder.path()).unwrap().unwrap();
	assert_eq!(catalog.manifest(), authored.as_slice());
	let mut scenes = SceneCatalog::default();
	assert!(catalog.mix().validate_scenes(&scenes).is_err());
	let garden = SceneId::new("garden").unwrap();
	scenes
		.insert(SceneDescriptor::new(garden.clone(), "scenes/garden.json").unwrap())
		.unwrap();
	catalog.mix().validate_scenes(&scenes).unwrap();
	assert_eq!(catalog.mix().category_percent(FeedbackBus::Collection), 100);
	assert_eq!(catalog.mix().category_percent(FeedbackBus::Mechanisms), 80);
	assert_eq!(
		catalog
			.mix()
			.scene_percent(&garden, FeedbackBus::Mechanisms),
		50
	);
	assert_eq!(combined_percent(75, 80, 50), Ok(30));
	assert_eq!(combined_percent(99, 15, 50), Ok(7));
	assert_eq!(combined_percent(99, 50, 50), Ok(25));
}

#[test]
fn rejects_wrong_schema_fields_bad_values_duplicates_and_oversized_mix() {
	let folder = Folder::new();
	fs::create_dir(folder.path().join("audio")).unwrap();
	let manifest = folder.path().join("audio/cues.json");
	for catalog in [
		r#"{"schema_version":1,"cues":[],"bus_percent":{}}"#,
		r#"{"schema_version":1,"cues":[],"scene_bus_percent":{}}"#,
		r#"{"schema_version":3,"cues":[]}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":{"speech":80}}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":{"mechanisms":101}}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":{"mechanisms":90.5}}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":{"mechanisms":null}}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":null}"#,
		r#"{"schema_version":2,"cues":[],"bus_percent":{"mechanisms":80,"mechanisms":20}}"#,
		r#"{"schema_version":2,"cues":[],"scene_bus_percent":{"bad id":{}}}"#,
		r#"{"schema_version":2,"cues":[],"scene_bus_percent":{"garden":{},"garden":{}}}"#,
	] {
		fs::write(&manifest, catalog).unwrap();
		assert!(load(folder.path()).is_err(), "accepted {catalog}");
	}
	let too_many = (0..17)
		.map(|index| (format!("scene-{index}"), json!({})))
		.collect::<serde_json::Map<String, Value>>();
	fs::write(
		&manifest,
		json!({"schema_version": 2, "cues": [], "scene_bus_percent": too_many}).to_string(),
	)
	.unwrap();
	assert!(load(folder.path()).is_err());
}
