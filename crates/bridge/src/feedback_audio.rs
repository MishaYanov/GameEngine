//! CPU-only preparation for optional, project-owned gameplay feedback.
use std::{
	collections::BTreeMap,
	fs,
	path::{Path, PathBuf},
	sync::Arc,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::asset_io::{contained_file, is_link, optional_contained_file, read_bounded};

const CATALOG: &str = "audio/cues.json";
const MAX_CATALOG_BYTES: usize = 4_096;
const HEADER_BYTES: usize = 44;
const MAX_PCM_BYTES: usize = 16_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackCue {
	KeyCollected,
	GateOpened,
	RunCompleted,
}

#[derive(Clone)]
pub struct CueClip(Arc<[u8]>);

impl CueClip {
	pub fn from_wave(bytes: Vec<u8>) -> Result<Self, &'static str> {
		validate_wave(&bytes)?;
		Ok(Self(bytes.into()))
	}

	pub fn bytes(&self) -> &[u8] {
		&self.0
	}

	/// Apply linear gain to the PCM samples while preserving the canonical WAV layout.
	pub fn scaled(&self, percent: u8) -> Result<Self, &'static str> {
		if percent > 100 {
			return Err("Feedback volume must be from 0 to 100");
		}
		if percent == 100 {
			return Ok(self.clone());
		}
		let mut bytes = self.bytes().to_vec();
		for sample in bytes[HEADER_BYTES..].chunks_exact_mut(2) {
			let original = i16::from_le_bytes([sample[0], sample[1]]);
			let value = (i32::from(original) * i32::from(percent) / 100) as i16;
			sample.copy_from_slice(&value.to_le_bytes());
		}
		Self::from_wave(bytes)
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueAssignment {
	pub event: FeedbackCue,
	pub path: String,
}

pub struct PreparedCueCatalog {
	manifest: Vec<u8>,
	events: BTreeMap<FeedbackCue, String>,
	clips: BTreeMap<String, CueClip>,
}

impl PreparedCueCatalog {
	/// Identity of the exact catalog bytes, including original formatting.
	pub fn digest(&self) -> String {
		digest(&self.manifest)
	}

	pub fn assignments(&self) -> impl Iterator<Item=CueAssignment> + '_ {
		self.events.iter().map(|(&event, path)| CueAssignment {
			event,
			path: path.clone(),
		})
	}

	pub fn manifest(&self) -> &[u8] {
		&self.manifest
	}

	pub fn clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		self.clips.get(self.events.get(&cue)?)
	}

	pub fn files(&self) -> impl Iterator<Item=(&'_ str, &'_ [u8])> {
		std::iter::once((CATALOG, self.manifest())).chain(
			self.clips
				.iter()
				.map(|(path, clip)| (path.as_str(), clip.bytes())),
		)
	}
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CueCatalog {
	schema_version: u32,
	cues: Vec<CueAssignment>,
}

/// Validates the portable catalog and all referenced clips before returning playable bytes.
pub fn load(asset_root: &Path) -> Result<Option<PreparedCueCatalog>, String> {
	let root = ordinary_asset_root(asset_root)?;
	read_manifest(&root)?
		.map(|manifest| {
			let catalog = serde_json::from_slice(&manifest)
				.map_err(|error| format!("Feedback cue catalog: {error}"))?;
			prepare(&root, manifest, catalog)
		})
		.transpose()
}

/// Checks assignments against actual project clips and prepares canonical editor output.
pub fn prepare_catalog(
	asset_root: &Path,
	mut cues: Vec<CueAssignment>,
) -> Result<PreparedCueCatalog, String> {
	if cues.len() > 3 || cues.iter().any(|cue| !valid_path(&cue.path)) {
		return Err("Assign at most three audio/cue-<slug>.wav files".into());
	}
	cues.sort_by_key(|cue| cue.event);
	let catalog = CueCatalog {
		schema_version: 1,
		cues,
	};
	let mut manifest = serde_json::to_vec_pretty(&catalog).map_err(|error| error.to_string())?;
	manifest.push(b'\n');
	prepare(&ordinary_asset_root(asset_root)?, manifest, catalog)
}

/// Reads and validates a single local, project-relative clip for an editor audition.
pub fn load_clip(asset_root: &Path, relative_path: &str) -> Result<CueClip, String> {
	prepare_clip(&ordinary_asset_root(asset_root)?, relative_path)
}

/// Reads the selected import file before it can be published into the project.
pub fn read_external_clip(path: &Path) -> Result<CueClip, String> {
	let bytes = read_bounded(path, HEADER_BYTES + MAX_PCM_BYTES)?;
	CueClip::from_wave(bytes).map_err(str::to_owned)
}

pub fn import_path(slug: &str) -> Result<String, String> {
	if !valid_slug(slug) {
		return Err("Choose a short filename using lowercase letters, digits or hyphens".into());
	}
	Ok(format!("audio/cue-{slug}.wav"))
}

/// Resolves an ordinary checked directory for the native editor writer.
pub fn ensure_audio_directory(asset_root: &Path) -> Result<PathBuf, String> {
	let root = ordinary_asset_root(asset_root)?;
	let audio = root.join("audio");
	match fs::create_dir(&audio) {
		Ok(()) => {}
		Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
		Err(error) => return Err(error.to_string()),
	}
	let metadata = fs::symlink_metadata(&audio).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Feedback audio directory must be an ordinary project directory".into());
	}
	let directory = audio.canonicalize().map_err(|error| error.to_string())?;
	if !directory.starts_with(&root) {
		return Err("Feedback audio directory escapes the asset root".into());
	}
	Ok(directory)
}

/// Compares the exact sidecar bytes without reading all referenced clips again.
pub fn catalog_digest(asset_root: &Path) -> Result<Option<String>, String> {
	Ok(read_manifest(&ordinary_asset_root(asset_root)?)?
		.as_deref()
		.map(digest))
}

fn ordinary_asset_root(asset_root: &Path) -> Result<PathBuf, String> {
	let metadata = fs::symlink_metadata(asset_root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Feedback audio asset root must be an ordinary directory".into());
	}
	asset_root.canonicalize().map_err(|error| error.to_string())
}

fn read_manifest(root: &Path) -> Result<Option<Vec<u8>>, String> {
	optional_contained_file(root, CATALOG)?
		.map(|path| read_bounded(&path, MAX_CATALOG_BYTES))
		.transpose()
}

fn digest(manifest: &[u8]) -> String {
	format!("{:x}", Sha256::digest(manifest))
}

fn prepare(
	root: &Path,
	manifest: Vec<u8>,
	catalog: CueCatalog,
) -> Result<PreparedCueCatalog, String> {
	if catalog.schema_version != 1 || catalog.cues.len() > 3 {
		return Err("Feedback cue catalog requires schema 1 and at most three events".into());
	}
	let mut events = BTreeMap::new();
	let mut clips = BTreeMap::new();
	for entry in catalog.cues {
		let CueAssignment { event, path } = entry;
		if events.insert(event, path.clone()).is_some() {
			return Err("Feedback cue catalog repeats an event".into());
		}
		if !clips.contains_key(&path) {
			let clip = prepare_clip(root, &path)?;
			clips.insert(path, clip);
		}
	}
	Ok(PreparedCueCatalog {
		manifest,
		events,
		clips,
	})
}

fn prepare_clip(root: &Path, path: &str) -> Result<CueClip, String> {
	if !valid_path(path) {
		return Err("Feedback cue path must match audio/cue-<slug>.wav".into());
	}
	read_external_clip(&contained_file(root, path)?).map_err(|error| format!("{path}: {error}"))
}

fn valid_path(path: &str) -> bool {
	path.strip_prefix("audio/cue-")
		.and_then(|name| name.strip_suffix(".wav"))
		.is_some_and(valid_slug)
}

fn valid_slug(slug: &str) -> bool {
	(1..=32).contains(&slug.len())
		&& slug
		.bytes()
		.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_wave(wave: &[u8]) -> Result<(), &'static str> {
	if !(HEADER_BYTES + 2..=HEADER_BYTES + MAX_PCM_BYTES).contains(&wave.len()) {
		return Err("Cue WAV has an invalid byte length");
	}
	let u16_at = |at| u16::from_le_bytes([wave[at], wave[at + 1]]);
	let u32_at = |at| u32::from_le_bytes([wave[at], wave[at + 1], wave[at + 2], wave[at + 3]]);
	let pcm_bytes = wave.len() - HEADER_BYTES;
	if &wave[..4] != b"RIFF"
		|| u32_at(4) as usize != wave.len() - 8
		|| &wave[8..16] != b"WAVEfmt "
		|| u32_at(16) != 16
		|| (u16_at(20), u16_at(22)) != (1, 1)
		|| (u32_at(24), u32_at(28)) != (16_000, 32_000)
		|| (u16_at(32), u16_at(34)) != (2, 16)
		|| &wave[36..40] != b"data"
		|| u32_at(40) as usize != pcm_bytes
		|| !pcm_bytes.is_multiple_of(2)
	{
		return Err("Cue WAV must be canonical 16 kHz, 16-bit mono PCM");
	}
	Ok(())
}
