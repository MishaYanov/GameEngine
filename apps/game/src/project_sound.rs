//! Standalone music and combat cues driven by committed project state.

use crate::player_controls::FeedbackMix;
use bridge::{feedback_audio::FeedbackBus, project_audio::ProjectAudio};
use gameplay_catalog::cinder_oath::{CinderProgress, Phase};
#[cfg(windows)]
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CombatCue {
	Swing,
	Hit,
	Hurt,
	Victory,
	Defeat,
}

impl CombatCue {
	pub(super) const ALL: [Self; 5] = [
		Self::Swing,
		Self::Hit,
		Self::Hurt,
		Self::Victory,
		Self::Defeat,
	];

	pub(super) fn id(self) -> &'static str {
		match self {
			Self::Swing => "swing",
			Self::Hit => "hit",
			Self::Hurt => "hurt",
			Self::Victory => "victory",
			Self::Defeat => "defeat",
		}
	}

	pub(super) fn bus(self) -> FeedbackBus {
		match self {
			Self::Swing | Self::Hit => FeedbackBus::Collection,
			Self::Hurt => FeedbackBus::Mechanisms,
			Self::Victory | Self::Defeat => FeedbackBus::Completion,
		}
	}

	pub(super) fn gain(self, mix: FeedbackMix) -> u8 {
		(u16::from(mix.effective_percent()) * u16::from(mix.category_percent(self.bus())) / 100)
			as u8
	}

	pub(super) fn caption(self) -> &'static str {
		match self {
			Self::Swing => "Sword swing",
			Self::Hit => "Sword impact",
			Self::Hurt => "Hit by the Warden",
			Self::Victory => "Victory",
			Self::Defeat => "Defeat",
		}
	}
}

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
	fn cue(self, after: Self) -> Option<CombatCue> {
		if self.phase != after.phase {
			match after.phase {
				Phase::Victory => return Some(CombatCue::Victory),
				Phase::Defeat => return Some(CombatCue::Defeat),
				Phase::Intro | Phase::Duel => {}
			}
		}
		if after.player_hits > self.player_hits {
			Some(CombatCue::Hurt)
		} else if after.enemy_hits > self.enemy_hits {
			Some(CombatCue::Hit)
		} else if after.swings > self.swings {
			Some(CombatCue::Swing)
		} else {
			None
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum PreviewStatus {
	#[default]
	Ready,
	Playing {
		poll_at: Instant,
		timeout: Instant,
	},
	Finished,
	Interrupted,
	SilentMix,
	MissingClip,
	OutputUnavailable,
}

impl PreviewStatus {
	#[cfg(windows)]
	const POLL_INTERVAL: Duration = Duration::from_millis(50);
	#[cfg(windows)]
	const TIMEOUT: Duration = Duration::from_secs(5);

	pub(super) fn busy(self) -> bool {
		matches!(self, Self::Playing { .. })
	}
}

#[derive(Default)]
pub(super) struct ProjectSound {
	asset: Option<ProjectAudio>,
	preview: PreviewStatus,
	#[cfg(windows)]
	audio: Option<winmm::SoundSession>,
}

impl ProjectSound {
	pub(super) fn new(asset: Option<ProjectAudio>, enabled: bool) -> Self {
		#[cfg(not(windows))]
		if enabled && asset.is_some() {
			engine_logger::info!("Project soundtrack playback requires Windows");
		}
		Self {
			asset,
			preview: PreviewStatus::Ready,
			#[cfg(windows)]
			audio: enabled.then(winmm::SoundSession::new),
		}
	}

	pub(super) fn has_soundtrack(&self) -> bool {
		self.asset.is_some()
	}

	pub(super) fn update(&mut self, playing: bool, active_window: bool, gain: u8) {
		if !active_window {
			self.interrupt_preview();
		}
		#[cfg(windows)]
		if let (Some(audio), Some(asset)) = (&mut self.audio, &self.asset) {
			audio.update(asset, playing, active_window, gain);
		}
		#[cfg(not(windows))]
		let _ = (playing, gain);
	}

	pub(super) fn emit_combat_change(
		&mut self,
		prior: CombatSound,
		after: CombatSound,
		mix: FeedbackMix,
	) -> Option<CombatCue> {
		let cue = prior.cue(after)?;
		#[cfg(windows)]
		if let (Some(audio), Some(clip)) = (
			&mut self.audio,
			self.asset.as_ref().and_then(|asset| asset.effect(cue.id())),
		) {
			audio.emit(cue.id(), clip.pcm_bytes(), cue.gain(mix));
		}
		#[cfg(not(windows))]
		let _ = mix;
		Some(cue)
	}

	pub(super) fn stop_effect(&mut self) {
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio {
			audio.stop_effect();
		}
		self.interrupt_preview();
	}

	pub(super) fn configured_cues(&self) -> Vec<CombatCue> {
		CombatCue::ALL
			.into_iter()
			.filter(|cue| {
				self.asset
					.as_ref()
					.is_some_and(|asset| asset.effect(cue.id()).is_some())
			})
			.collect()
	}

	pub(super) fn preview_status(&self) -> PreviewStatus {
		self.preview
	}

	pub(super) fn audition(&mut self, cue: CombatCue, mix: FeedbackMix, now: Instant) {
		if self.preview.busy() {
			return;
		}
		let Some(clip) = self.asset.as_ref().and_then(|asset| asset.effect(cue.id())) else {
			self.preview = PreviewStatus::MissingClip;
			return;
		};
		let gain = cue.gain(mix);
		if gain == 0 {
			self.preview = PreviewStatus::SilentMix;
			return;
		}
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio
			&& audio.can_audition()
			&& audio.preview.play(clip.pcm_bytes(), gain, false)
		{
			self.preview = PreviewStatus::Playing {
				poll_at: now + PreviewStatus::POLL_INTERVAL,
				timeout: now + PreviewStatus::TIMEOUT,
			};
			return;
		}
		#[cfg(not(windows))]
		let _ = (clip, now);
		self.preview = PreviewStatus::OutputUnavailable;
	}

	pub(super) fn poll_preview(&mut self, now: Instant) -> bool {
		#[cfg(windows)]
		if let PreviewStatus::Playing { poll_at, timeout } = self.preview
			&& now >= poll_at
		{
			let finished = self
				.audio
				.as_ref()
				.is_some_and(|audio| audio.preview.finished());
			if finished || now >= timeout {
				self.stop_preview_lane();
				self.preview = if finished {
					PreviewStatus::Finished
				} else {
					PreviewStatus::OutputUnavailable
				};
				return true;
			}
			self.preview = PreviewStatus::Playing {
				poll_at: (now + PreviewStatus::POLL_INTERVAL).min(timeout),
				timeout,
			};
		}
		#[cfg(not(windows))]
		let _ = now;
		false
	}

	pub(super) fn preview_deadline(&self) -> Option<Instant> {
		match self.preview {
			PreviewStatus::Playing { poll_at, .. } => Some(poll_at),
			_ => None,
		}
	}

	pub(super) fn reset_preview(&mut self) {
		self.stop_preview_lane();
		self.preview = PreviewStatus::Ready;
	}

	fn interrupt_preview(&mut self) {
		if self.preview.busy() {
			self.stop_preview_lane();
			self.preview = PreviewStatus::Interrupted;
		}
	}

	fn stop_preview_lane(&mut self) {
		#[cfg(windows)]
		if let Some(audio) = &mut self.audio {
			audio.stop_preview();
		}
	}
}

#[cfg(windows)]
mod winmm {
	use std::{
		fmt, io,
		mem::size_of,
		os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
		ptr,
	};

	use bridge::project_audio::ProjectAudio;
	use windows_sys::Win32::{
		Foundation::WAIT_OBJECT_0,
		Media::{
			Audio::{
				CALLBACK_EVENT, HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR,
				WHDR_BEGINLOOP, WHDR_DONE, WHDR_ENDLOOP, waveOutClose, waveOutOpen, waveOutPause,
				waveOutPrepareHeader, waveOutReset, waveOutRestart, waveOutUnprepareHeader,
				waveOutWrite,
			},
			MMSYSERR_NOERROR,
		},
		System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
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
		playback: Playback,
		music_gain: Option<u8>,
		music: OutputLane,
		effect: OutputLane,
		pub(super) preview: OutputLane,
	}

	impl SoundSession {
		pub(super) fn new() -> Self {
			Self {
				playback: Playback::default(),
				music_gain: None,
				music: OutputLane::new("soundtrack", false),
				effect: OutputLane::new("sound effects", false),
				preview: OutputLane::new("audio-check preview", true),
			}
		}

		pub(super) fn update(
			&mut self,
			asset: &ProjectAudio,
			playing: bool,
			active_window: bool,
			gain: u8,
		) {
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
				let music = asset.music();
				if self.music.play(music.pcm_bytes(), next.gain, true) {
					self.music_gain = Some(next.gain);
					engine_logger::debug!("Project soundtrack loop started");
				}
			} else if !prior.active_window {
				self.music.with_output(WaveOutput::resume);
			}
		}

		pub(super) fn emit(&mut self, id: &str, samples: &[u8], gain: u8) {
			if self.playback.audible() && gain > 0 && self.effect.play(samples, gain, false) {
				engine_logger::debug!("Project combat sound dispatched: {id}");
			}
		}

		pub(super) fn can_audition(&self) -> bool {
			self.playback.active_window
		}

		pub(super) fn stop_preview(&mut self) {
			self.preview.with_output(WaveOutput::stop);
		}

		pub(super) fn stop_effect(&mut self) {
			self.effect.with_output(WaveOutput::stop);
		}
	}

	pub(super) struct OutputLane {
		label: &'static str,
		report_completion: bool,
		device: Option<WaveOutput>,
		unavailable: bool,
	}

	impl OutputLane {
		fn new(label: &'static str, report_completion: bool) -> Self {
			Self {
				label,
				report_completion,
				device: None,
				unavailable: false,
			}
		}

		pub(super) fn play(&mut self, bytes: &[u8], gain: u8, repeat: bool) -> bool {
			if self.unavailable {
				return false;
			}
			if self.device.is_none() {
				match WaveOutput::open(self.report_completion) {
					Ok(device) => self.device = Some(device),
					Err(error) => {
						self.disable(error);
						return false;
					}
				}
			}
			self.with_output(|device| device.submit(bytes, gain, repeat))
		}

		pub(super) fn finished(&self) -> bool {
			self.device.as_ref().is_some_and(|device| device.finished())
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
				"{} returned status {}",
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

	struct CompletionEvent(OwnedHandle);

	impl CompletionEvent {
		fn new() -> Result<Self, WinmmError> {
			// A private, initially unsignaled auto-reset event serves one queued
			// preview buffer. The output owns its handle through waveOutClose.
			let handle = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
			if handle.is_null() {
				return Err(Self::last_error("CreateEventW"));
			}
			Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
		}

		fn reset(&self) -> Result<(), WinmmError> {
			// The owned handle stays valid for the synchronous reset call.
			if unsafe { ResetEvent(self.0.as_raw_handle()) } == 0 {
				return Err(Self::last_error("ResetEvent"));
			}
			Ok(())
		}

		fn signaled(&self) -> bool {
			// Zero timeout preserves the event loop's non-blocking polling.
			(unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) }) == WAIT_OBJECT_0
		}

		fn last_error(operation: &'static str) -> WinmmError {
			WinmmError {
				operation,
				status: io::Error::last_os_error()
					.raw_os_error()
					.unwrap_or_default() as u32,
			}
		}
	}

	struct WaveOutput {
		handle: HWAVEOUT,
		buffer: Option<PlayingBuffer>,
		completion: Option<CompletionEvent>,
	}

	impl WaveOutput {
		fn open(report_completion: bool) -> Result<Self, WinmmError> {
			let completion = report_completion.then(CompletionEvent::new).transpose()?;
			let (callback, flags) = completion.as_ref().map_or((0, 0), |event| {
				(event.0.as_raw_handle() as usize, CALLBACK_EVENT)
			});
			let mut handle = ptr::null_mut();
			// FORMAT and the output pointer are valid for this call. WinMM can
			// signal the owned event until the output is closed.
			check("waveOutOpen", unsafe {
				waveOutOpen(&mut handle, WAVE_MAPPER, &FORMAT, callback, 0, flags)
			})?;
			Ok(Self {
				handle,
				buffer: None,
				completion,
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
			if let Some(event) = &self.completion {
				event.reset()?;
			}
			check("waveOutWrite", unsafe {
				waveOutWrite(self.handle, header, HEADER_SIZE)
			})?;
			self.resume()
		}

		fn finished(&self) -> bool {
			self.buffer.as_ref().is_some_and(|buffer| {
				if !self
					.completion
					.as_ref()
					.is_some_and(CompletionEvent::signaled)
				{
					return false;
				}
				// WinMM signaled a state change. Check the current playback
				// header as documented for CALLBACK_EVENT.
				let flags = unsafe { ptr::addr_of!((*buffer.header).dwFlags).read_volatile() };
				flags & WHDR_DONE != 0
			})
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
			let closed = unsafe { waveOutClose(self.handle) } == MMSYSERR_NOERROR;
			if !closed && let Some(event) = self.completion.take() {
				std::mem::forget(event);
			}
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
			let mut music = WaveOutput::open(false).unwrap();
			let mut effect = WaveOutput::open(false).unwrap();
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

		#[test]
		#[ignore = "requires an available Windows waveform output device"]
		fn preview_event_reports_completion_and_can_be_reused_after_cancel() {
			let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../projects/cinder-oath/assets");
			let audio = bridge::project_audio::load(&root).unwrap().unwrap();
			let clip = audio.effect("hit").unwrap().pcm_bytes();
			let mut preview = WaveOutput::open(true).unwrap();
			preview.submit(clip, 0, false).unwrap();
			preview.stop().unwrap();
			assert!(!preview.finished());

			preview.submit(clip, 0, false).unwrap();
			let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
			while !preview.finished() {
				assert!(
					std::time::Instant::now() < deadline,
					"preview completion was not signaled"
				);
				std::thread::sleep(std::time::Duration::from_millis(20));
			}
			preview.stop().unwrap();
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
		assert_eq!(duel.cue(swung), Some(CombatCue::Swing));
		assert_eq!(swung.cue(hit), Some(CombatCue::Hit));
		assert_eq!(hit.cue(victory), Some(CombatCue::Victory));
		assert_eq!(hit.cue(hurt), Some(CombatCue::Hurt));
		assert_eq!(hurt.cue(defeat), Some(CombatCue::Defeat));
		assert_eq!(victory.cue(victory), None);
		assert_eq!(victory.cue(duel), None);
	}
}
