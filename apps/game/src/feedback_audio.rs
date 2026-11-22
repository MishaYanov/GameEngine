//! Prepared audio for committed gameplay cues and Player Controls auditions.
use bridge::feedback_audio::{CueClip, FeedbackBus, FeedbackCue, PreparedCueCatalog};
use runtime::scene_catalog::SceneId;
#[cfg(windows)]
use std::{
	sync::{
		Arc, Weak,
		atomic::{AtomicU64, Ordering},
		mpsc::{SyncSender, sync_channel},
	},
	thread,
};

use super::player_controls::FeedbackMix;

static GATE_TONE: &[u8] = include_bytes!("gate-open.wav");

#[derive(Clone, Copy)]
pub(super) struct Facts {
	pub key: bool,
	pub gate: bool,
	pub complete: bool,
}

impl Facts {
	pub fn rising(self, after: Self) -> impl Iterator<Item = FeedbackCue> {
		[
			(!self.key && after.key, FeedbackCue::KeyCollected),
			(!self.gate && after.gate, FeedbackCue::GateOpened),
			(!self.complete && after.complete, FeedbackCue::RunCompleted),
		]
		.into_iter()
		.filter_map(|(changed, cue)| changed.then_some(cue))
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CueGainFactors {
	pub master_percent: u8,
	pub local_percent: u8,
	pub project_percent: u8,
	pub scene_percent: u8,
}

impl CueGainFactors {
	fn resolved_percent(self) -> u8 {
		resolved_bus_percent(
			self.master_percent,
			self.local_percent,
			self.project_percent,
			self.scene_percent,
		)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CueClipSource {
	Authored,
	BuiltIn,
	Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CueGainDetails {
	pub factors: CueGainFactors,
	pub resolved_percent: u8,
	pub clip: CueClipSource,
}

pub(super) enum CueAuditionOutcome {
	#[cfg(windows)]
	Submitted,
	SilentMix,
	MissingClip,
	OutputUnavailable,
}

/// The audio menu owns this lifetime. Queued auditions stop when it closes.
#[derive(Default)]
pub(super) struct AuditionScope {
	#[cfg(windows)]
	menu: Arc<()>,
}

#[derive(Default)]
pub(super) struct FeedbackAudio {
	catalog: Option<PreparedCueCatalog>,
	legacy_gate: Option<CueClip>,
	mix: PreparedMix,
	scene: Option<SceneId>,
	#[cfg(windows)]
	worker: Option<Worker>,
}

impl FeedbackAudio {
	pub fn new(
		catalog: Option<PreparedCueCatalog>,
		enabled: bool,
		settings: FeedbackMix,
		startup: &SceneId,
	) -> Result<Self, String> {
		let legacy_gate = catalog
			.is_none()
			.then(|| CueClip::from_wave(GATE_TONE.to_vec()).expect("embedded PCM must be valid"));
		#[cfg(not(windows))]
		let _ = enabled;
		let mut audio = Self {
			catalog,
			legacy_gate,
			mix: PreparedMix::default(),
			scene: Some(startup.clone()),
			#[cfg(windows)]
			worker: None,
		};
		let prepared = audio.prepare_mix(settings)?;
		audio.commit_mix(prepared);
		#[cfg(windows)]
		{
			audio.worker = enabled.then(Worker::start).flatten();
		}
		Ok(audio)
	}

	fn original_clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		match &self.catalog {
			Some(catalog) => catalog.clip(cue),
			None if cue == FeedbackCue::GateOpened => self.legacy_gate.as_ref(),
			None => None,
		}
	}

	pub fn prepare_mix(&self, settings: FeedbackMix) -> Result<Option<PreparedMix>, String> {
		self.prepare_for_scene(settings, self.scene.as_ref())
	}

	pub fn prepare_scene_mix(
		&self,
		settings: FeedbackMix,
		scene: &SceneId,
	) -> Result<Option<PreparedMix>, String> {
		self.prepare_for_scene(settings, Some(scene))
	}

	fn prepare_for_scene(
		&self,
		settings: FeedbackMix,
		scene: Option<&SceneId>,
	) -> Result<Option<PreparedMix>, String> {
		let settings = settings.validate()?;
		let gains = FeedbackCue::ALL.map(|cue| {
			self.gain_factors(settings, scene, cue.bus())
				.resolved_percent()
		});
		if self.mix.gains == gains && self.mix.settings == settings {
			return Ok(None);
		}
		let mut mix = PreparedMix {
			settings,
			gains,
			..PreparedMix::default()
		};
		for cue in FeedbackCue::ALL {
			let slot = cue_slot(cue);
			if gains[slot] == self.mix.gains[slot] {
				mix.clips[slot] = self.mix.clips[slot].clone();
			} else if gains[slot] != 0 {
				mix.clips[slot] = self
					.original_clip(cue)
					.map(|clip| clip.scaled(gains[slot]))
					.transpose()?;
			}
		}
		Ok(Some(mix))
	}

	fn gain_factors(
		&self,
		settings: FeedbackMix,
		scene: Option<&SceneId>,
		bus: FeedbackBus,
	) -> CueGainFactors {
		let authored = self.catalog.as_ref().map(PreparedCueCatalog::mix);
		CueGainFactors {
			master_percent: settings.effective_percent(),
			local_percent: settings.category_percent(bus),
			project_percent: authored.map_or(100, |mix| mix.category_percent(bus)),
			scene_percent: authored
				.zip(scene)
				.map_or(100, |(mix, id)| mix.scene_percent(id, bus)),
		}
	}

	pub fn commit_scene_mix(&mut self, scene: SceneId, prepared: Option<PreparedMix>) {
		if self.scene.as_ref() != Some(&scene) {
			self.invalidate_audition();
		}
		self.scene = Some(scene);
		self.commit_mix(prepared);
	}

	pub fn commit_mix(&mut self, prepared: Option<PreparedMix>) {
		if let Some(mix) = prepared {
			if self.mix.gains != mix.gains {
				self.invalidate();
			} else if self.mix.settings != mix.settings {
				self.invalidate_audition();
			}
			self.mix = mix;
		}
	}

	pub fn cue_details(&self, bus: FeedbackBus) -> CueGainDetails {
		let cue = cue_for_bus(bus);
		let clip = if self.original_clip(cue).is_none() {
			CueClipSource::Missing
		} else if self.catalog.is_some() {
			CueClipSource::Authored
		} else {
			CueClipSource::BuiltIn
		};
		CueGainDetails {
			factors: self.gain_factors(self.mix.settings, self.scene.as_ref(), bus),
			resolved_percent: self.mix.gains[cue_slot(cue)],
			clip,
		}
	}

	pub fn audition(&self, bus: FeedbackBus, scope: &AuditionScope) -> CueAuditionOutcome {
		let cue = cue_for_bus(bus);
		if self.original_clip(cue).is_none() {
			return CueAuditionOutcome::MissingClip;
		}
		let Some(clip) = self.clip(cue) else {
			return CueAuditionOutcome::SilentMix;
		};
		#[cfg(windows)]
		if self
			.worker
			.as_ref()
			.is_some_and(|worker| worker.enqueue_audition(clip.clone(), scope))
		{
			return CueAuditionOutcome::Submitted;
		}
		#[cfg(not(windows))]
		let _ = (clip, scope);
		CueAuditionOutcome::OutputUnavailable
	}

	fn clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		self.mix.clips[cue_slot(cue)].as_ref()
	}

	#[cfg(all(test, windows))]
	pub(super) fn test_gain(&self, cue: FeedbackCue) -> u8 {
		self.mix.gains[cue_slot(cue)]
	}

	#[cfg(all(test, windows))]
	pub(super) fn test_audition_counts(&self) -> (u64, u64) {
		let worker = self.worker.as_ref().unwrap();
		(
			worker.epochs.auditions_completed.load(Ordering::Acquire),
			worker.epochs.auditions_played.load(Ordering::Relaxed),
		)
	}

	pub fn emit(&self, cue: FeedbackCue) {
		if let Some(clip) = self.clip(cue) {
			#[cfg(windows)]
			if let Some(worker) = &self.worker {
				worker.enqueue(clip.clone());
			}
			#[cfg(not(windows))]
			let _ = clip;
		}
	}

	pub fn invalidate(&self) {
		#[cfg(windows)]
		if let Some(worker) = &self.worker {
			worker.epochs.gameplay.fetch_add(1, Ordering::Release);
		}
		self.invalidate_audition();
	}

	fn invalidate_audition(&self) {
		#[cfg(windows)]
		if let Some(worker) = &self.worker {
			worker.epochs.audition.fetch_add(1, Ordering::Release);
		}
	}
}

fn resolved_bus_percent(local: u8, user_bus: u8, authored: u8, scene: u8) -> u8 {
	let product = u32::from(local) * u32::from(user_bus) * u32::from(authored) * u32::from(scene);
	((product + 500_000) / 1_000_000) as u8
}

#[derive(Default)]
pub(super) struct PreparedMix {
	settings: FeedbackMix,
	gains: [u8; FeedbackCue::ALL.len()],
	clips: [Option<CueClip>; FeedbackCue::ALL.len()],
}

fn cue_for_bus(bus: FeedbackBus) -> FeedbackCue {
	match bus {
		FeedbackBus::Collection => FeedbackCue::KeyCollected,
		FeedbackBus::Mechanisms => FeedbackCue::GateOpened,
		FeedbackBus::Completion => FeedbackCue::RunCompleted,
	}
}

fn cue_slot(cue: FeedbackCue) -> usize {
	match cue {
		FeedbackCue::KeyCollected => 0,
		FeedbackCue::GateOpened => 1,
		FeedbackCue::RunCompleted => 2,
	}
}

impl Drop for FeedbackAudio {
	fn drop(&mut self) {
		self.invalidate();
	}
}

#[cfg(windows)]
#[derive(Default)]
struct PlaybackEpochs {
	gameplay: AtomicU64,
	audition: AtomicU64,
	#[cfg(test)]
	auditions_completed: AtomicU64,
	#[cfg(test)]
	auditions_played: AtomicU64,
}

#[cfg(windows)]
struct QueuedAudition {
	epoch: u64,
	clip: CueClip,
	menu: Weak<()>,
}

#[cfg(windows)]
impl QueuedAudition {
	fn is_current(&self, epochs: &PlaybackEpochs) -> bool {
		self.epoch == epochs.audition.load(Ordering::Acquire) && self.menu.strong_count() != 0
	}
}

#[cfg(windows)]
struct Worker {
	output: SyncSender<(u64, CueClip)>,
	auditions: SyncSender<QueuedAudition>,
	epochs: Arc<PlaybackEpochs>,
	wake: SyncSender<()>,
}

#[cfg(windows)]
impl Worker {
	fn start() -> Option<Self> {
		let (output, input) = sync_channel::<(u64, CueClip)>(4);
		let (auditions, previews) = sync_channel::<QueuedAudition>(1);
		let (wake, ready) = sync_channel(1);
		let epochs = Arc::new(PlaybackEpochs::default());
		let worker_epochs = Arc::clone(&epochs);
		let result = thread::Builder::new().name("game-audio".into()).spawn(move || {
			let mut gameplay_failed = false;
			let mut audition_failed = false;
			loop {
				if let Ok((queued_epoch, clip)) = input.try_recv() {
					if queued_epoch == worker_epochs.gameplay.load(Ordering::Acquire)
						&& !play_clip(&clip)
						&& !gameplay_failed
					{
						gameplay_failed = true;
						engine_logger::warn!(target: "game::audio", "Gameplay audio device did not play the cue");
					}
				} else if let Ok(request) = previews.try_recv() {
					if request.is_current(&worker_epochs) {
						let played = play_clip(&request.clip);
						#[cfg(test)]
						{
							if played {
								worker_epochs.auditions_played.fetch_add(1, Ordering::Relaxed);
							}
							worker_epochs.auditions_completed.fetch_add(1, Ordering::Release);
						}
						if !played && !audition_failed {
							audition_failed = true;
							engine_logger::warn!(target: "game::audio", "Audio device did not play the Controls cue test");
						}
					}
				} else if ready.recv().is_err() {
					// Each submission signals this mailbox; dropping its sender ends playback.
					break;
				}
			}
		});
		if result.is_err() {
			engine_logger::warn!(target: "game::audio", "Could not start gameplay audio worker");
			return None;
		}
		Some(Self {
			output,
			auditions,
			epochs,
			wake,
		})
	}

	fn enqueue(&self, clip: CueClip) {
		// A stalled device may drop a new cue. Simulation never waits for audio.
		let epoch = self.epochs.gameplay.load(Ordering::Acquire);
		if self.output.try_send((epoch, clip)).is_ok() {
			let _ = self.wake.try_send(());
		}
	}

	fn enqueue_audition(&self, clip: CueClip, scope: &AuditionScope) -> bool {
		let queued = self
			.auditions
			.try_send(QueuedAudition {
				epoch: self.epochs.audition.load(Ordering::Acquire),
				clip,
				menu: Arc::downgrade(&scope.menu),
			})
			.is_ok();
		if queued {
			let _ = self.wake.try_send(());
		}
		queued
	}
}

#[cfg(windows)]
fn play_clip(clip: &CueClip) -> bool {
	use std::ffi::c_void;

	#[link(name = "winmm")]
	unsafe extern "system" {
		fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> i32;
	}

	const SND_NODEFAULT: u32 = 0x0002;
	const SND_MEMORY: u32 = 0x0004;
	// SAFETY: SND_MEMORY uses a WAVE byte buffer with no string interpretation.
	// Synchronous playback keeps the owned validated buffer alive until return.
	unsafe {
		PlaySoundW(
			clip.bytes().as_ptr().cast(),
			std::ptr::null_mut(),
			SND_NODEFAULT | SND_MEMORY,
		) != 0
	}
}

#[cfg(test)]
#[path = "../tests/unit/feedback_audio.rs"]
mod tests;
