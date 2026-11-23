//! Device-independent decisions for Audio Sources at committed scene boundaries.
use crate::{
	EntityKey, SceneData,
	animation::MarkerId,
	asset_catalog::AssetId,
	command::PlayStatusDto,
	components::{
		AudioEventBinding, AudioSourceBus, AudioSourcePlayback, AudioSourceSpatial, ComponentId,
		LegacyCombatCue,
	},
	gameplay_events::Signal,
	world::RuntimeWorld,
};
use serde::{Deserialize, Serialize};

pub const MAX_SCENE_VOICES: usize = 16;
const MAX_AUDIO_ONSETS_PER_STEP: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneAudioOwner {
	pub run_id: String,
	pub scene_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneAudioVoice {
	pub entity: EntityKey,
	pub component: ComponentId,
	pub clip: AssetId,
	pub gain: f32,
	pub looping: bool,
	pub bus: AudioSourceBus,
	pub spatial: AudioSourceSpatial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioOnsetCause {
	GameplaySignal {
		signal: Signal,
	},
	AnimationMarker {
		clip: AssetId,
		marker: MarkerId,
		playback: u64,
		loop_index: u64,
	},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommittedAudioOnset {
	pub voice: SceneAudioVoice,
	pub tick: u64,
	pub cause: AudioOnsetCause,
	pub replaces_cue: Option<LegacyCombatCue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneAudioOp {
	Start {
		owner: SceneAudioOwner,
		voice: SceneAudioVoice,
	},
	Trigger {
		owner: SceneAudioOwner,
		onset: CommittedAudioOnset,
	},
	StopOwner {
		owner: SceneAudioOwner,
	},
}

#[derive(Default)]
pub struct SceneAudioState {
	owner: Option<SceneAudioOwner>,
}

impl SceneAudioOwner {
	pub fn from_play(status: &PlayStatusDto) -> Self {
		Self {
			run_id: status.run_id.clone(),
			scene_generation: status.scene_generation,
		}
	}
}

impl SceneAudioState {
	pub fn sync(&mut self, next: Option<(SceneAudioOwner, &SceneData)>) -> Vec<SceneAudioOp> {
		if self.owner.as_ref() == next.as_ref().map(|(owner, _)| owner) {
			return Vec::new();
		}
		let mut changes = Vec::new();
		if let Some(owner) = self.owner.take() {
			changes.push(SceneAudioOp::StopOwner { owner });
		}
		if let Some((owner, scene)) = next {
			changes.extend(
				entry_voices(scene)
					.into_iter()
					.map(|voice| SceneAudioOp::Start {
						owner: owner.clone(),
						voice,
					}),
			);
			self.owner = Some(owner);
		}
		changes
	}

	pub fn owns(&self, owner: &SceneAudioOwner) -> bool {
		self.owner.as_ref() == Some(owner)
	}
}

pub(crate) fn available_onsets(output: &[CommittedAudioOnset], tick: u64) -> usize {
	MAX_AUDIO_ONSETS_PER_STEP.saturating_sub(
		output
			.iter()
			.rev()
			.take_while(|onset| onset.tick == tick)
			.count(),
	)
}

pub(crate) fn record_onset(
	world: &RuntimeWorld,
	key: &EntityKey,
	tick: u64,
	cause: AudioOnsetCause,
	output: &mut Vec<CommittedAudioOnset>,
) {
	if available_onsets(output, tick) == 0
		|| output
			.iter()
			.rev()
			.take_while(|onset| onset.tick == tick)
			.any(|onset| onset.voice.entity == *key && onset.cause == cause)
	{
		return;
	}
	let Some(entity) = world.entity_index(key).map(|i| &world.data().entities[i]) else {
		return;
	};
	let Some(source) = entity.audio_source.as_ref().filter(|source| source.enabled) else {
		return;
	};
	let settings = &source.settings;
	let matches = match (&settings.event, &cause) {
		(
			Some(AudioEventBinding::GameplaySignal { signal: expected }),
			AudioOnsetCause::GameplaySignal { signal },
		) => expected == signal,
		(
			Some(AudioEventBinding::AnimationMarker {
				clip: expected_clip,
				marker: expected_marker,
			}),
			AudioOnsetCause::AnimationMarker { clip, marker, .. },
		) => expected_clip == clip && expected_marker == marker,
		_ => false,
	};
	let Some(clip) = settings.clip.as_ref() else {
		return;
	};
	if settings.playback != AudioSourcePlayback::OnEvent || !matches {
		return;
	}
	output.push(CommittedAudioOnset {
		voice: SceneAudioVoice {
			entity: key.clone(),
			component: source.id.clone(),
			clip: clip.asset.clone(),
			gain: settings.gain,
			looping: false,
			bus: settings.bus,
			spatial: settings.spatial,
		},
		tick,
		cause,
		replaces_cue: settings.replaces_cue,
	});
}

pub fn entry_voices(scene: &SceneData) -> Vec<SceneAudioVoice> {
	use std::collections::BTreeMap;

	let mut music = None;
	let mut effects = BTreeMap::new();
	for entity in &scene.entities {
		let Some(source) = entity.audio_source.as_ref() else {
			continue;
		};
		let settings = &source.settings;
		if !source.enabled || settings.playback != AudioSourcePlayback::OnSceneEnter {
			continue;
		}
		let Some(clip) = settings.clip.as_ref() else {
			continue;
		};
		let candidate = (&entity.key, source, clip);
		match settings.bus {
			AudioSourceBus::Music => {
				if music.is_none_or(|(current, _, _)| &entity.key < current) {
					music = Some(candidate);
				}
			}
			AudioSourceBus::Effects => {
				effects.insert(&entity.key, candidate);
				if effects.len() > MAX_SCENE_VOICES {
					effects.pop_last();
				}
			}
		}
	}
	music
		.into_iter()
		.chain(effects.into_values())
		.take(MAX_SCENE_VOICES)
		.map(|(key, source, clip)| SceneAudioVoice {
			entity: key.clone(),
			component: source.id.clone(),
			clip: clip.asset.clone(),
			gain: source.settings.gain,
			looping: source.settings.r#loop,
			bus: source.settings.bus,
			spatial: source.settings.spatial,
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		audio_clip::AudioClipRef,
		components::{AudioSourceComponent, AudioSourceSettings},
		scene_data::{CURRENT_SCHEMA_VERSION, EntityData},
	};

	#[test]
	fn scene_entries_are_capped_ordered_and_reset_with_the_owner() {
		let clip_id = AssetId::generate();
		let make = |key: String, bus, enabled| {
			let mut entity = EntityData::new(EntityKey::new(&key).unwrap(), key, "unused.glb");
			entity.audio_source = Some(Box::new(AudioSourceComponent {
				id: ComponentId::new("source").unwrap(),
				enabled,
				settings: AudioSourceSettings {
					clip: Some(AudioClipRef {
						asset: clip_id.clone(),
					}),
					bus,
					r#loop: true,
					..Default::default()
				},
			}));
			entity
		};
		let mut entities = (0..32)
			.rev()
			.map(|i| make(format!("effect-{i:02}"), AudioSourceBus::Effects, true))
			.collect::<Vec<_>>();
		entities.extend([
			make("music-b".into(), AudioSourceBus::Music, true),
			make("music-a".into(), AudioSourceBus::Music, true),
			make("disabled".into(), AudioSourceBus::Effects, false),
		]);
		let scene = SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			entities,
			bindings: Default::default(),
			camera: Default::default(),
			default_game_camera: None,
			lights: Vec::new(),
			organization: Default::default(),
		};
		let mut state = SceneAudioState::default();
		let first = SceneAudioOwner {
			run_id: "initial".into(),
			scene_generation: 1,
		};
		let second = SceneAudioOwner {
			run_id: "restart".into(),
			scene_generation: 1,
		};
		let starts = state.sync(Some((first.clone(), &scene)));
		assert_eq!(starts.len(), MAX_SCENE_VOICES);
		assert!(
			matches!(&starts[0], SceneAudioOp::Start { voice, .. } if voice.entity.as_str() == "music-a")
		);
		assert!(
			matches!(&starts[15], SceneAudioOp::Start { voice, .. } if voice.entity.as_str() == "effect-14")
		);
		assert!(state.sync(Some((first.clone(), &scene))).is_empty());
		let changes = state.sync(Some((second.clone(), &scene)));
		assert!(matches!(&changes[0], SceneAudioOp::StopOwner { owner } if owner == &first));
		assert_eq!(changes.len(), MAX_SCENE_VOICES + 1);
		assert!(!state.owns(&first));
		assert!(state.owns(&second));
		assert_eq!(
			state.sync(None),
			[SceneAudioOp::StopOwner { owner: second }]
		);
	}
}
