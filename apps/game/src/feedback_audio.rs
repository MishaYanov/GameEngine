//! Presentation-only audio for events that have committed to a game run.
use bridge::feedback_audio::{CueClip, FeedbackCue, PreparedCueCatalog};
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
	pub fn rising(self, after: Self) -> impl Iterator<Item=FeedbackCue> {
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
	#[cfg(windows)]
	worker: Option<Worker>,
}

impl FeedbackAudio {
	pub fn new(
		catalog: Option<PreparedCueCatalog>,
		enabled: bool,
		gain_percent: u8,
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
		if gain_percent > 100 {
			return Err("Feedback volume must be from 0 to 100".into());
		}
		if self.mix.gain_percent == gain_percent {
			return Ok(None);
		}
		let mut mix = PreparedMix {
			gain_percent,
			..PreparedMix::default()
		};
		if gain_percent != 0 {
			for cue in [
				FeedbackCue::KeyCollected,
				FeedbackCue::GateOpened,
				FeedbackCue::RunCompleted,
			] {
				mix.clips[cue_slot(cue)] = self
					.original_clip(cue)
					.map(|clip| clip.scaled(gain_percent))
					.transpose()?;
			}
		}
		Ok(Some(mix))
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
	gain_percent: u8,
	clips: [Option<CueClip>; 3],
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
