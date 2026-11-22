//! Presentation-only audio for events that have committed to a game run.
use bridge::feedback_audio::{CueClip, FeedbackCue, PreparedCueCatalog, combined_percent};
use runtime::scene_catalog::SceneId;
#[cfg(windows)]
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
	mpsc::{SyncSender, sync_channel},
};

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
		gain_percent: u8,
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
		let prepared = audio.prepare_mix(gain_percent)?;
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

	pub fn prepare_mix(&self, gain_percent: u8) -> Result<Option<PreparedMix>, String> {
		self.prepare_for_scene(gain_percent, self.scene.as_ref())
	}

	pub fn prepare_scene_mix(
		&self,
		gain_percent: u8,
		scene: &SceneId,
	) -> Result<Option<PreparedMix>, String> {
		self.prepare_for_scene(gain_percent, Some(scene))
	}

	fn prepare_for_scene(
		&self,
		gain_percent: u8,
		scene: Option<&SceneId>,
	) -> Result<Option<PreparedMix>, String> {
		let mut gains = [0; FeedbackCue::ALL.len()];
		let authored = self.catalog.as_ref().map(PreparedCueCatalog::mix);
		for cue in FeedbackCue::ALL {
			let category = authored.map_or(100, |catalog| catalog.category_percent(cue.bus()));
			let scene = authored
				.zip(scene)
				.map_or(100, |(catalog, id)| catalog.scene_percent(id, cue.bus()));
			gains[cue_slot(cue)] = combined_percent(gain_percent, category, scene)?;
		}
		if self.mix.gains == gains {
			return Ok(None);
		}
		let mut mix = PreparedMix {
			gains,
			..PreparedMix::default()
		};
		for cue in FeedbackCue::ALL {
			let slot = cue_slot(cue);
			if gains[slot] != 0 {
				mix.clips[slot] = self
					.original_clip(cue)
					.map(|clip| clip.scaled(gains[slot]))
					.transpose()?;
			}
		}
		Ok(Some(mix))
	}

	pub fn commit_scene_mix(&mut self, scene: SceneId, prepared: Option<PreparedMix>) {
		self.scene = Some(scene);
		self.commit_mix(prepared);
	}

	pub fn commit_mix(&mut self, prepared: Option<PreparedMix>) {
		if let Some(mix) = prepared {
			self.invalidate();
			self.mix = mix;
		}
	}

	fn clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		self.mix.clips[cue_slot(cue)].as_ref()
	}

	#[cfg(all(test, windows))]
	pub(super) fn test_gain(&self, cue: FeedbackCue) -> u8 {
		self.mix.gains[cue_slot(cue)]
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
			worker.epoch.fetch_add(1, Ordering::Relaxed);
		}
	}
}

#[derive(Default)]
pub(super) struct PreparedMix {
	gains: [u8; FeedbackCue::ALL.len()],
	clips: [Option<CueClip>; FeedbackCue::ALL.len()],
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
struct Worker {
	output: SyncSender<(u64, CueClip)>,
	epoch: Arc<AtomicU64>,
}

#[cfg(windows)]
impl Worker {
	fn start() -> Option<Self> {
		type Item = (u64, CueClip);
		let (output, input) = sync_channel::<Item>(4);
		let epoch = Arc::new(AtomicU64::new(0));
		let current_epoch = Arc::clone(&epoch);
		let result = std::thread::Builder::new()
			.name("game-audio".into())
			.spawn(move || {
				let mut reported = false;
				while let Ok((queued_epoch, clip)) = input.recv() {
					if queued_epoch == current_epoch.load(Ordering::Relaxed)
						&& !play_clip(&clip)
						&& !reported
					{
						reported = true;
						engine_logger::warn!(target: "game::audio", "Gameplay audio device did not play the cue");
					}
				}
			});
		if result.is_err() {
			engine_logger::warn!(target: "game::audio", "Could not start gameplay audio worker");
			return None;
		}
		Some(Self { output, epoch })
	}

	fn enqueue(&self, clip: CueClip) {
		// A stalled device may drop a new cue. Simulation never waits for audio.
		let _ = self
			.output
			.try_send((self.epoch.load(Ordering::Relaxed), clip));
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
