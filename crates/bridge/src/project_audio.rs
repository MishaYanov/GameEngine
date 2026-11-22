//! Validated project-owned music and short gameplay sounds.

use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use serde::Deserialize;

use crate::{
	asset_io::{contained_file, is_link, optional_contained_file, read_bounded},
	feedback_audio::{HEADER_BYTES, validate_wave},
};

const MANIFEST: &str = "audio/soundtrack.json";
const MAX_MUSIC_BYTES: usize = 2 * 1024 * 1024;
const MAX_EFFECT_BYTES: usize = 32_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Soundtrack {
	schema_version: u32,
	music: String,
	#[serde(default)]
	effects: BTreeMap<String, String>,
}

pub struct ProjectAudio {
	manifest: Vec<u8>,
	soundtrack: Soundtrack,
	waves: BTreeMap<String, WaveClip>,
}

impl ProjectAudio {
	pub fn music(&self) -> &WaveClip {
		&self.waves[&self.soundtrack.music]
	}

	pub fn effect(&self, id: &str) -> Option<&WaveClip> {
		let path = self.soundtrack.effects.get(id)?;
		self.waves.get(path)
	}

	pub fn files(&self) -> impl Iterator<Item=(&str, &[u8])> {
		std::iter::once((MANIFEST, self.manifest.as_slice())).chain(
			self.waves
				.iter()
				.map(|(name, clip)| (name.as_str(), clip.0.as_ref())),
		)
	}
}

#[derive(Clone)]
pub struct WaveClip(Arc<[u8]>);

impl WaveClip {
	pub fn pcm_bytes(&self) -> &[u8] {
		&self.0[HEADER_BYTES..]
	}
}

/// Returns canonical 16 kHz mono PCM files and their exact authored bytes for export.
pub fn load(asset_root: &Path) -> Result<Option<ProjectAudio>, String> {
	let metadata = fs::symlink_metadata(asset_root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || is_link(&metadata) {
		return Err("Soundtrack asset root must be an ordinary directory".into());
	}
	let root = asset_root
		.canonicalize()
		.map_err(|error| error.to_string())?;
	let Some(path) = optional_contained_file(&root, MANIFEST)? else {
		return Ok(None);
	};
	let manifest = read_bounded(&path, 8 * 1024)?;
	let soundtrack: Soundtrack = serde_json::from_slice(&manifest)
		.map_err(|error| format!("Soundtrack manifest: {error}"))?;
	if soundtrack.schema_version != 1 || soundtrack.effects.len() > 8 {
		return Err("Expected soundtrack schema 1 with up to eight sound effects".into());
	}
	let mut waves = BTreeMap::new();
	let music = &soundtrack.music;
	waves.insert(music.clone(), read_wave(&root, music, MAX_MUSIC_BYTES)?);
	for (name, path) in &soundtrack.effects {
		if !valid_name(name) || path == music {
			return Err("A sound effect requires its own WAV and a short lowercase ID".into());
		}
		if !waves.contains_key(path) {
			waves.insert(path.clone(), read_wave(&root, path, MAX_EFFECT_BYTES)?);
		}
	}
	Ok(Some(ProjectAudio {
		manifest,
		soundtrack,
		waves,
	}))
}

fn valid_name(name: &str) -> bool {
	(1..=48).contains(&name.len())
		&& name
		.bytes()
		.all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-')
}

fn read_wave(root: &Path, path: &str, max_pcm_bytes: usize) -> Result<WaveClip, String> {
	if !path
		.strip_prefix("audio/")
		.and_then(|name| name.strip_suffix(".wav"))
		.is_some_and(valid_name)
	{
		return Err("Soundtrack paths must name a WAV directly inside audio/".into());
	}
	let bytes = read_bounded(&contained_file(root, path)?, HEADER_BYTES + max_pcm_bytes)?;
	validate_wave(&bytes, max_pcm_bytes).map_err(|error| format!("{path}: {error}"))?;
	Ok(WaveClip(bytes.into()))
}
