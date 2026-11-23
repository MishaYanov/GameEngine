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

	pub(super) fn update(
		&mut self,
		playing: bool,
		active_window: bool,
		gain: u8,
		authored_music: bool,
	) {
		if !active_window {
			self.interrupt_preview();
		}
		#[cfg(windows)]
		if let (Some(audio), Some(asset)) = (&mut self.audio, &self.asset) {
			audio.update(asset, playing, active_window, gain, authored_music);
		}
		#[cfg(not(windows))]
		let _ = (playing, gain, authored_music);
	}

	pub(super) fn emit_combat_change(
		&mut self,
		prior: CombatSound,
		after: CombatSound,
		mix: FeedbackMix,
		replaced: impl Fn(CombatCue) -> bool,
	) -> Option<CombatCue> {
		let cue = prior.cue(after)?;
		#[cfg(windows)]
		if !replaced(cue)
			&& let (Some(audio), Some(clip)) = (
				&mut self.audio,
				self.asset.as_ref().and_then(|asset| asset.effect(cue.id())),
			) {
			audio.emit(cue.id(), clip.pcm_bytes(), cue.gain(mix));
		}
		#[cfg(not(windows))]
		let _ = (mix, replaced);
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
	use bridge::project_audio::ProjectAudio;
	#[cfg(test)]
	use runtime::scene_audio::scaled_pcm;
	use runtime::scene_audio::{WaveOutput, WinmmError};

	#[derive(Clone, Copy, Default, PartialEq, Eq)]
	struct Playback {
		playing: bool,
		active_window: bool,
		gain: u8,
		authored_music: bool,
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
			authored_music: bool,
		) {
			let next = Playback {
				playing,
				active_window,
				gain,
				authored_music,
			};
			if self.playback == next {
				return;
			}
			let prior = std::mem::replace(&mut self.playback, next);
			if !next.audible() || prior.gain != next.gain {
				self.stop_effect();
			}
			if !next.playing || next.gain == 0 || next.authored_music {
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
