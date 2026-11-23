//! Bounded project audio files for the authored Audio Clip asset kind.
use crate::{
	asset_io,
	feedback_audio::{HEADER_BYTES, validate_wave},
};
use engine_core::audio_clip::{
	AUDIO_CLIP_SAMPLE_RATE_HZ, AudioClipInfo, MAX_AUDIO_CLIP_PCM_BYTES, MAX_AUDIO_CLIP_WAV_BYTES,
};
use std::{fs, path::Path};

/// Audition is a one-shot excerpt, independent of authored loop playback.
pub const MAX_AUDIO_AUDITION_WAV_BYTES: usize =
	HEADER_BYTES + 4 * AUDIO_CLIP_SAMPLE_RATE_HZ as usize;

#[derive(Debug)]
pub struct AudioClip {
	wav: Vec<u8>,
	info: AudioClipInfo,
}

impl AudioClip {
	pub fn info(&self) -> AudioClipInfo {
		self.info
	}
	pub fn wav_bytes(&self) -> &[u8] {
		&self.wav
	}
	pub fn pcm_bytes(&self) -> &[u8] {
		&self.wav[HEADER_BYTES..]
	}
	pub fn into_wav(self) -> Vec<u8> {
		self.wav
	}

	pub fn into_audition_wav(self) -> Vec<u8> {
		let mut wave = self.wav;
		if wave.len() > MAX_AUDIO_AUDITION_WAV_BYTES {
			wave.truncate(MAX_AUDIO_AUDITION_WAV_BYTES);
			let riff_bytes = (wave.len() - 8) as u32;
			let pcm_bytes = (wave.len() - HEADER_BYTES) as u32;
			wave[4..8].copy_from_slice(&riff_bytes.to_le_bytes());
			wave[40..44].copy_from_slice(&pcm_bytes.to_le_bytes());
		}
		wave
	}
}

/// Verify the format and sample count without allocating decoded samples.
pub fn inspect(wav: &[u8]) -> Result<AudioClipInfo, String> {
	if wav.len() > MAX_AUDIO_CLIP_WAV_BYTES {
		return Err("Audio Clip exceeds the 30-second PCM WAV budget".into());
	}
	validate_wave(wav, MAX_AUDIO_CLIP_PCM_BYTES).map_err(|_| {
		"Audio Clip requires a canonical 16 kHz, signed 16-bit mono PCM WAV".to_string()
	})?;
	Ok(AudioClipInfo {
		sample_rate_hz: AUDIO_CLIP_SAMPLE_RATE_HZ,
		frames: ((wav.len() - HEADER_BYTES) / 2) as u32,
		channels: 1,
	})
}

pub fn read_external(path: &Path) -> Result<AudioClip, String> {
	if !path.is_absolute()
		|| !path
			.extension()
			.and_then(|extension| extension.to_str())
			.is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
	{
		return Err("Choose an absolute .wav Audio Clip source file".into());
	}
	let wav = asset_io::read_bounded(path, MAX_AUDIO_CLIP_WAV_BYTES)?;
	let info = inspect(&wav)?;
	Ok(AudioClip { wav, info })
}

pub fn read_project(asset_root: &Path, relative: &str) -> Result<AudioClip, String> {
	let metadata = fs::symlink_metadata(asset_root).map_err(|error| error.to_string())?;
	if !metadata.is_dir() || asset_io::is_link(&metadata) {
		return Err("Audio Clip asset root must be an ordinary directory".into());
	}
	let root = asset_root
		.canonicalize()
		.map_err(|error| error.to_string())?;
	read_external(&asset_io::contained_file(&root, relative)?)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compatible_pcm_is_measured_and_excess_samples_are_rejected() {
		let short = include_bytes!("../../../projects/cinder-oath/assets/audio/blade-hit.wav");
		let info = inspect(short).unwrap();
		assert_eq!(info.frames as usize * 2, short.len() - HEADER_BYTES);
		assert_eq!(info.sample_rate_hz, 16_000);
		assert_eq!(info.channels, 1);
		let mut boundary = short.to_vec();
		boundary.resize(MAX_AUDIO_CLIP_WAV_BYTES, 0);
		boundary[4..8].copy_from_slice(&((MAX_AUDIO_CLIP_WAV_BYTES - 8) as u32).to_le_bytes());
		boundary[40..44].copy_from_slice(&(MAX_AUDIO_CLIP_PCM_BYTES as u32).to_le_bytes());
		assert_eq!(
			inspect(&boundary).unwrap().frames,
			engine_core::audio_clip::MAX_AUDIO_CLIP_FRAMES
		);
		let clip = AudioClip {
			info: inspect(&boundary).unwrap(),
			wav: boundary.clone(),
		};
		let excerpt = clip.into_audition_wav();
		assert_eq!(excerpt.len(), MAX_AUDIO_AUDITION_WAV_BYTES);
		assert_eq!(inspect(&excerpt).unwrap().frames, 32_000);
		boundary.push(0);
		assert!(inspect(&boundary).unwrap_err().contains("30-second"));
		let mut wrong_format = short.to_vec();
		wrong_format[34..36].copy_from_slice(&24_u16.to_le_bytes());
		assert!(inspect(&wrong_format).unwrap_err().contains("16-bit mono"));
		assert!(inspect(&short[..short.len() - 1]).is_err());
	}
}
