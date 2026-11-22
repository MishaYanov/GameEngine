//! Standalone music and combat cues driven by committed project state.

use bridge::project_audio::ProjectAudio;
use gameplay_catalog::cinder_oath::{CinderProgress, Phase};

#[derive(Clone, Copy)]
pub(super) struct CombatSound {
	phase: Phase,
	swings: u32,
	enemy_hits: u32,
	player_hits: u32,
}

impl From<&CinderProgress> for CombatSound {
	fn from(progress: &CinderProgress) -> Self {
		Self {
			phase: progress.phase(),
			swings: progress.swings(),
			enemy_hits: progress.enemy_hits(),
			player_hits: progress.player_hits(),
		}
	}
}

impl CombatSound {
	fn cue(self, after: Self) -> Option<&'static str> {
		if self.phase != after.phase {
			match after.phase {
				Phase::Victory => return Some("victory"),
				Phase::Defeat => return Some("defeat"),
				Phase::Intro | Phase::Duel => {}
			}
		}
		if after.player_hits > self.player_hits {
			Some("hurt")
		} else if after.enemy_hits > self.enemy_hits {
			Some("hit")
		} else if after.swings > self.swings {
			Some("swing")
		} else {
			None
		}
	}
}

#[derive(Default)]
pub(super) struct ProjectSound {
	#[cfg(windows)]
	audio: Option<winmm::SoundSession>,
}

impl ProjectSound {
	pub(super) fn new(asset: Option<ProjectAudio>, enabled: bool) -> Self {
		#[cfg(windows)]
		{
			Self {
				audio: asset.filter(|_| enabled).map(winmm::SoundSession::new),
			}
		}
		#[cfg(not(windows))]
		{
			if enabled && asset.is_some() {
				engine_logger::info!("Project soundtrack playback requires Windows");
			}
			Self {}
		}
	}

	pub(super) fn has_soundtrack(&self) -> bool {
		#[cfg(windows)]
		return self.audio.is_some();
		#[cfg(not(windows))]
		false
	}

	pub(super) fn update(&mut self, playing: bool, active_window: bool, gain: u8) {
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio {
			audio.update(playing, active_window, gain);
		}
		#[cfg(not(windows))]
		let _ = (playing, active_window, gain);
	}

	pub(super) fn emit_combat_change(&mut self, prior: CombatSound, after: CombatSound) {
		let Some(cue) = prior.cue(after) else { return };
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio {
			audio.emit(cue);
		}
		#[cfg(not(windows))]
		let _ = cue;
	}

	pub(super) fn stop_effect(&mut self) {
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio {
			audio.stop_effect();
		}
	}
}

#[cfg(windows)]
mod winmm {
	use std::{fmt, mem::size_of, ptr};

	use bridge::project_audio::ProjectAudio;
	use windows_sys::Win32::Media::{
		Audio::{
			HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WHDR_BEGINLOOP,
			WHDR_ENDLOOP, waveOutClose, waveOutOpen, waveOutPause, waveOutPrepareHeader,
			waveOutReset, waveOutRestart, waveOutUnprepareHeader, waveOutWrite,
		},
		MMSYSERR_NOERROR,
	};

	const HEADER_SIZE: u32 = size_of::<WAVEHDR>() as u32;
	const FORMAT: WAVEFORMATEX = WAVEFORMATEX {
		wFormatTag: WAVE_FORMAT_PCM as u16,
		nChannels: 1,
		nSamplesPerSec: 16_000,
		nAvgBytesPerSec: 32_000,
		nBlockAlign: 2,
		wBitsPerSample: 16,
		cbSize: 0,
	};

	#[derive(Clone, Copy, Default, PartialEq, Eq)]
	struct Playback {
		playing: bool,
		active_window: bool,
		gain: u8,
	}

	impl Playback {
		fn audible(self) -> bool {
			self.playing && self.active_window && self.gain != 0
		}
	}

	pub(super) struct SoundSession {
		asset: ProjectAudio,
		playback: Playback,
		music_gain: Option<u8>,
		music: OutputLane,
		effect: OutputLane,
	}

	impl SoundSession {
		pub(super) fn new(asset: ProjectAudio) -> Self {
			Self {
				asset,
				playback: Playback::default(),
				music_gain: None,
				music: OutputLane::new("soundtrack"),
				effect: OutputLane::new("sound effects"),
			}
		}

		pub(super) fn update(&mut self, playing: bool, active_window: bool, gain: u8) {
			let next = Playback {
				playing,
				active_window,
				gain,
			};
			if self.playback == next {
				return;
			}
			let prior = std::mem::replace(&mut self.playback, next);
			if !next.audible() || prior.gain != next.gain {
				self.stop_effect();
			}
			if !next.playing || next.gain == 0 {
				self.music.with_output(WaveOutput::stop);
				self.music_gain = None;
			} else if !next.active_window {
				if prior.audible() {
					self.music.with_output(WaveOutput::pause);
				}
			} else if self.music_gain != Some(next.gain) {
				let music = self.asset.music();
				if self.music.play(music.pcm_bytes(), next.gain, true) {
					self.music_gain = Some(next.gain);
					engine_logger::debug!("Project soundtrack loop started");
				}
			} else if !prior.active_window {
				self.music.with_output(WaveOutput::resume);
			}
		}

		pub(super) fn emit(&mut self, id: &str) {
			if self.playback.audible()
				&& let Some(clip) = self.asset.effect(id)
				&& self
				.effect
				.play(clip.pcm_bytes(), self.playback.gain, false)
			{
				engine_logger::debug!("Project combat sound dispatched: {id}");
			}
		}

		pub(super) fn stop_effect(&mut self) {
			self.effect.with_output(WaveOutput::stop);
		}
	}

	struct OutputLane {
		label: &'static str,
		device: Option<WaveOutput>,
		unavailable: bool,
	}

	impl OutputLane {
		fn new(label: &'static str) -> Self {
			Self {
				label,
				device: None,
				unavailable: false,
			}
		}

		fn play(&mut self, bytes: &[u8], gain: u8, repeat: bool) -> bool {
			if self.unavailable {
				return false;
			}
			if self.device.is_none() {
				match WaveOutput::open() {
					Ok(device) => self.device = Some(device),
					Err(error) => {
						self.disable(error);
						return false;
					}
				}
			}
			self.with_output(|device| device.submit(bytes, gain, repeat))
		}

		fn with_output(
			&mut self,
			command: impl FnOnce(&mut WaveOutput) -> Result<(), WinmmError>,
		) -> bool {
			let Some(device) = &mut self.device else {
				return false;
			};
			if let Err(error) = command(device) {
				self.disable(error);
				return false;
			}
			true
		}

		fn disable(&mut self, error: WinmmError) {
			engine_logger::warn!("Project {} unavailable: {error}", self.label);
			self.unavailable = true;
			self.device = None;
		}
	}

	#[derive(Debug)]
	struct WinmmError {
		operation: &'static str,
		status: u32,
	}

	impl fmt::Display for WinmmError {
		fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
			write!(
				formatter,
				"{} returned WinMM status {}",
				self.operation, self.status
			)
		}
	}

	struct PlayingBuffer {
		header: *mut WAVEHDR,
		pcm: *mut [u8],
	}

	impl PlayingBuffer {
		fn new(pcm: Box<[u8]>) -> Self {
			let len = pcm.len();
			let pcm = Box::into_raw(pcm);
			let header = Box::into_raw(Box::new(WAVEHDR {
				lpData: pcm.cast::<u8>(),
				dwBufferLength: len as u32,
				..WAVEHDR::default()
			}));
			Self { header, pcm }
		}
	}

	impl Drop for PlayingBuffer {
		fn drop(&mut self) {
			// The queue owner drops this only before successful preparation or
			// after successful unprepare. Both pointers come from Box::into_raw.
			unsafe {
				drop(Box::from_raw(self.header));
				drop(Box::from_raw(self.pcm));
			}
		}
	}

	struct WaveOutput {
		handle: HWAVEOUT,
		buffer: Option<PlayingBuffer>,
	}

	impl WaveOutput {
		fn open() -> Result<Self, WinmmError> {
			let mut handle = ptr::null_mut();
			// FORMAT and the handle pointer are valid for this call. CALLBACK_NULL
			// keeps arbitrary driver threads out of application event handling.
			check("waveOutOpen", unsafe {
				waveOutOpen(&mut handle, WAVE_MAPPER, &FORMAT, 0, 0, 0)
			})?;
			Ok(Self {
				handle,
				buffer: None,
			})
		}

		fn submit(&mut self, samples: &[u8], gain: u8, repeat: bool) -> Result<(), WinmmError> {
			self.stop()?;
			let buffer = PlayingBuffer::new(scaled_pcm(samples, gain));
			let header = buffer.header;
			// Both allocations use raw ownership until stop() has reset and
			// unprepared the driver buffer. WinMM updates WAVEHDR flags.
			check("waveOutPrepareHeader", unsafe {
				waveOutPrepareHeader(self.handle, header, HEADER_SIZE)
			})?;
			if repeat {
				// WinMM permits setting loop fields after preparation and before
				// queueing, when playback cannot write the header.
				unsafe {
					(*header).dwFlags |= WHDR_BEGINLOOP | WHDR_ENDLOOP;
					(*header).dwLoops = u32::MAX;
				}
			}
			self.buffer = Some(buffer);
			check("waveOutWrite", unsafe {
				waveOutWrite(self.handle, header, HEADER_SIZE)
			})?;
			self.resume()
		}

		fn pause(&mut self) -> Result<(), WinmmError> {
			// The live handle belongs exclusively to this output lane.
			check("waveOutPause", unsafe { waveOutPause(self.handle) })
		}

		fn resume(&mut self) -> Result<(), WinmmError> {
			// Restart is a no-op on an output that is already running.
			check("waveOutRestart", unsafe { waveOutRestart(self.handle) })
		}

		fn stop(&mut self) -> Result<(), WinmmError> {
			let Some(buffer) = &self.buffer else {
				return Ok(());
			};
			// A successful reset returns every queued buffer for this handle.
			// Free neither allocation until unprepare has relinquished the header.
			check("waveOutReset", unsafe { waveOutReset(self.handle) })?;
			check("waveOutUnprepareHeader", unsafe {
				waveOutUnprepareHeader(self.handle, buffer.header, HEADER_SIZE)
			})?;
			self.buffer = None;
			Ok(())
		}
	}

	impl Drop for WaveOutput {
		fn drop(&mut self) {
			let unprepared = self.stop().is_ok();
			// The handle remains valid until close returns. If cleanup failed
			// and the driver could still own the buffer, leave that allocation
			// live for the process lifetime. The failing lane is never reused.
			let _ = unsafe { waveOutClose(self.handle) };
			if !unprepared && let Some(buffer) = self.buffer.take() {
				std::mem::forget(buffer);
			}
		}
	}

	fn scaled_pcm(samples: &[u8], gain: u8) -> Box<[u8]> {
		if gain == 100 {
			return samples.into();
		}
		let scale = i32::from(gain.min(100));
		samples
			.chunks_exact(2)
			.flat_map(|bytes| {
				let sample = i32::from(i16::from_le_bytes([bytes[0], bytes[1]]));
				((sample * scale / 100) as i16).to_le_bytes()
			})
			.collect()
	}

	fn check(operation: &'static str, status: u32) -> Result<(), WinmmError> {
		if status == MMSYSERR_NOERROR {
			Ok(())
		} else {
			Err(WinmmError { operation, status })
		}
	}

	#[cfg(test)]
	mod tests {
		use super::*;

		#[test]
		fn scales_signed_pcm_and_preserves_full_gain() {
			let input = [-32768_i16, -640, 0, 640, 32767]
				.into_iter()
				.flat_map(i16::to_le_bytes)
				.collect::<Vec<_>>();
			assert_eq!(&*scaled_pcm(&input, 100), &input);
			let half = [-16384_i16, -320, 0, 320, 16383]
				.into_iter()
				.flat_map(i16::to_le_bytes)
				.collect::<Vec<_>>();
			assert_eq!(&*scaled_pcm(&input, 50), &half);
		}

		#[test]
		#[ignore = "requires an available Windows waveform output device"]
		fn driver_accepts_a_loop_and_one_shot_on_separate_outputs() {
			let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../projects/cinder-oath/assets");
			let audio = bridge::project_audio::load(&root).unwrap().unwrap();
			let mut music = WaveOutput::open().unwrap();
			let mut effect = WaveOutput::open().unwrap();
			// Exercise the actual project clips silently so the test can run
			// alongside a live Player on the same machine.
			music.submit(audio.music().pcm_bytes(), 0, true).unwrap();
			effect
				.submit(audio.effect("hit").unwrap().pcm_bytes(), 0, false)
				.unwrap();
			music.pause().unwrap();
			music.resume().unwrap();
			effect.stop().unwrap();
			music.stop().unwrap();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn combat_cues_follow_committed_changes_and_prioritize_the_outcome() {
		let duel = CombatSound {
			phase: Phase::Duel,
			swings: 0,
			enemy_hits: 0,
			player_hits: 0,
		};
		let swung = CombatSound { swings: 1, ..duel };
		let hit = CombatSound {
			enemy_hits: 1,
			..swung
		};
		let victory = CombatSound {
			phase: Phase::Victory,
			enemy_hits: 2,
			..hit
		};
		let hurt = CombatSound {
			player_hits: 1,
			..hit
		};
		let defeat = CombatSound {
			phase: Phase::Defeat,
			player_hits: 2,
			..hurt
		};
		assert_eq!(duel.cue(swung), Some("swing"));
		assert_eq!(swung.cue(hit), Some("hit"));
		assert_eq!(hit.cue(victory), Some("victory"));
		assert_eq!(hit.cue(hurt), Some("hurt"));
		assert_eq!(hurt.cue(defeat), Some("defeat"));
		assert_eq!(victory.cue(victory), None);
		assert_eq!(victory.cue(duel), None);
	}
}
