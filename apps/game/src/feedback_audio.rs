//! Prepared project cue audio for Player Controls auditions.
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
		#[cfg(not(windows))]
		let _ = enabled;
		let mut audio = Self {
			catalog,
			mix: PreparedMix::default(),
			scene: Some(startup.clone()),
			#[cfg(windows)]
			worker: None,
		};
		let prepared = audio.prepare_mix(settings)?;
		audio.commit_mix(prepared);
		#[cfg(windows)]
		{
			audio.worker = (enabled && audio.catalog.is_some())
				.then(Worker::start)
				.flatten();
		}
		Ok(audio)
	}

	fn original_clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		self.catalog.as_ref()?.clip(cue)
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
			self.invalidate();
		}
		self.scene = Some(scene);
		self.commit_mix(prepared);
	}

	pub fn commit_mix(&mut self, prepared: Option<PreparedMix>) {
		if let Some(mix) = prepared {
			self.invalidate();
			self.mix = mix;
		}
	}

	pub fn cue_details(&self, bus: FeedbackBus) -> CueGainDetails {
		let cue = cue_for_bus(bus);
		let clip = if self.original_clip(cue).is_some() {
			CueClipSource::Authored
		} else {
			CueClipSource::Missing
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

	pub fn invalidate(&self) {
		#[cfg(windows)]
		if let Some(worker) = &self.worker {
			worker.epoch.fetch_add(1, Ordering::Release);
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
struct QueuedAudition {
	epoch: u64,
	clip: CueClip,
	menu: Weak<()>,
}

#[cfg(windows)]
impl QueuedAudition {
	fn is_current(&self, epoch: &AtomicU64) -> bool {
		self.epoch == epoch.load(Ordering::Acquire) && self.menu.strong_count() != 0
	}
}

#[cfg(windows)]
struct Worker {
	auditions: SyncSender<QueuedAudition>,
	epoch: Arc<AtomicU64>,
}

#[cfg(windows)]
impl Worker {
	fn start() -> Option<Self> {
		let (auditions, previews) = sync_channel::<QueuedAudition>(1);
		let epoch = Arc::new(AtomicU64::new(0));
		let worker_epoch = Arc::clone(&epoch);
		let result = thread::Builder::new().name("cue-audition".into()).spawn(move || {
			let mut reported_failure = false;
			while let Ok(request) = previews.recv() {
				if request.is_current(&worker_epoch)
					&& !play_clip(&request.clip)
					&& !reported_failure
				{
					reported_failure = true;
					engine_logger::warn!(target: "game::audio", "Audio device did not play the Controls cue test");
				}
			}
		});
		if result.is_err() {
			engine_logger::warn!(target: "game::audio", "Could not start the Controls cue audition worker");
			return None;
		}
		Some(Self { auditions, epoch })
	}

	fn enqueue_audition(&self, clip: CueClip, scope: &AuditionScope) -> bool {
		self.auditions
			.try_send(QueuedAudition {
				epoch: self.epoch.load(Ordering::Acquire),
				clip,
				menu: Arc::downgrade(&scope.menu),
			})
			.is_ok()
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
