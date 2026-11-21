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
	#[cfg(windows)]
	worker: Option<Worker>,
}

impl FeedbackAudio {
	pub fn new(catalog: Option<PreparedCueCatalog>, enabled: bool) -> Self {
		let legacy_gate = catalog
			.is_none()
			.then(|| CueClip::from_wave(GATE_TONE.to_vec()).expect("embedded PCM must be valid"));
		#[cfg(not(windows))]
		let _ = enabled;
		Self {
			catalog,
			legacy_gate,
			#[cfg(windows)]
			worker: enabled.then(Worker::start).flatten(),
		}
	}

	fn clip(&self, cue: FeedbackCue) -> Option<&CueClip> {
		match &self.catalog {
			Some(catalog) => catalog.clip(cue),
			None if cue == FeedbackCue::GateOpened => self.legacy_gate.as_ref(),
			None => None,
		}
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
