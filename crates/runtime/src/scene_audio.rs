//! Host-owned playback for committed Audio Sources.
#[cfg(windows)]
mod winmm;

#[cfg(windows)]
pub use winmm::{WaveOutput, WinmmError, scaled_pcm};

use std::path::Path;

use engine_core::{SceneData, scene_audio::SceneAudioState};

#[cfg(windows)]
use engine_core::scene_audio::SceneAudioOp;
pub use engine_core::scene_audio::SceneAudioOwner;

#[derive(Debug, Clone, Copy)]
pub struct SceneAudioMix {
	pub master_percent: u8,
	pub effects_percent: u8,
	pub music_percent: u8,
}

impl Default for SceneAudioMix {
	fn default() -> Self {
		Self {
			master_percent: 100,
			effects_percent: 100,
			music_percent: 100,
		}
	}
}

#[cfg(windows)]
struct Voice {
	spec: engine_core::scene_audio::SceneAudioVoice,
	state: VoiceState,
}

#[cfg(windows)]
enum VoiceState {
	Ready(bridge::audio_clip::AudioClip),
	Playing { output: WaveOutput, gain: f32 },
	Finished,
}

#[derive(Default)]
pub struct NativeSceneAudio {
	state: SceneAudioState,
	#[cfg(windows)]
	voices: Vec<Voice>,
	#[cfg(windows)]
	audible: bool,
	authored_music: bool,
}

impl NativeSceneAudio {
	pub fn synchronize(
		&mut self,
		next: Option<(SceneAudioOwner, &SceneData, &Path)>,
		audible: bool,
		mix: SceneAudioMix,
		is_current: impl Fn(&SceneAudioOwner) -> bool,
	) {
		let changes = self.state.sync(
			next.as_ref()
				.map(|(owner, scene, _)| (owner.clone(), *scene)),
		);
		#[cfg(not(windows))]
		let _ = (&changes, audible, mix, is_current);
		#[cfg(windows)]
		{
			self.apply_changes(changes, next.as_ref().map(|(_, _, path)| *path));
			if let Some((owner, _, _)) = next {
				if !self.state.owns(&owner) || !is_current(&owner) {
					self.clear();
					return;
				}
				self.update_voices(audible, mix);
			}
		}
	}

	pub fn authored_music(&self) -> bool {
		self.authored_music
	}

	#[cfg(windows)]
	fn clear(&mut self) {
		self.state = SceneAudioState::default();
		self.voices.clear();
		self.audible = false;
		self.authored_music = false;
	}

	#[cfg(windows)]
	fn apply_changes(&mut self, changes: Vec<SceneAudioOp>, asset_root: Option<&Path>) {
		use crate::assets::project_assets;

		if changes.is_empty() {
			return;
		}
		// Drop device handles for the outgoing owner before loading the new scene.
		self.voices.clear();
		self.authored_music = false;
		self.audible = false;
		let Some(root) = asset_root else { return };
		if !changes
			.iter()
			.any(|change| matches!(change, SceneAudioOp::Start { .. }))
		{
			return;
		}
		let catalog = match project_assets::load_catalog(root) {
			Ok(catalog) => catalog,
			Err(error) => {
				engine_logger::warn!("Scene audio catalog is unavailable: {error}");
				return;
			}
		};
		for change in changes {
			let SceneAudioOp::Start { voice, .. } = change else {
				continue;
			};
			let clip = catalog
				.by_id(&voice.clip)
				.ok_or_else(|| "Audio Clip is absent from the asset catalog".to_owned())
				.and_then(|asset| project_assets::load_audio_clip(root, asset));
			match clip {
				Ok(clip) => {
					self.authored_music |=
						voice.bus == engine_core::components::AudioSourceBus::Music;
					self.voices.push(Voice {
						spec: voice,
						state: VoiceState::Ready(clip),
					});
				}
				Err(error) => {
					engine_logger::warn!("Scene Audio Source {} cannot play: {error}", voice.entity.as_str())
				}
			}
		}
	}

	#[cfg(windows)]
	fn update_voices(&mut self, audible: bool, mix: SceneAudioMix) {
		use engine_core::components::AudioSourceBus;

		let was_audible = std::mem::replace(&mut self.audible, audible);
		for voice in &mut self.voices {
			let bus_percent = match voice.spec.bus {
				AudioSourceBus::Effects => mix.effects_percent,
				AudioSourceBus::Music => mix.music_percent,
			};
			let gain =
				voice.spec.gain * f32::from(mix.master_percent) * f32::from(bus_percent) / 10_000.0;
			let result = match &mut voice.state {
				VoiceState::Ready(clip) if audible => {
					WaveOutput::open_stereo(true).and_then(|mut output| {
						let hardware_gain = output.set_gain([gain; 2]).is_ok();
						let pcm_gain = if hardware_gain {
							100
						} else {
							(gain * 100.0).round() as u8
						};
						output.submit(clip.pcm_bytes(), pcm_gain, voice.spec.looping)?;
						Ok(Some(VoiceState::Playing { output, gain }))
					})
				}
				VoiceState::Playing {
					output,
					gain: previous,
				} => {
					if !voice.spec.looping && output.finished() {
						Ok(Some(VoiceState::Finished))
					} else {
						if *previous != gain {
							let _ = output.set_gain([gain; 2]);
							*previous = gain;
						}
						match (was_audible, audible) {
							(true, false) => output.pause(),
							(false, true) => output.resume(),
							_ => Ok(()),
						}
						.map(|()| None)
					}
				}
				_ => Ok(None),
			};
			match result {
				Ok(Some(state)) => voice.state = state,
				Ok(None) => {}
				Err(error) => {
					voice.state = VoiceState::Finished;
					engine_logger::warn!(
						"Scene Audio Source {} has no playback device: {error}",
						voice.spec.entity.as_str()
					);
				}
			}
		}
	}
}
