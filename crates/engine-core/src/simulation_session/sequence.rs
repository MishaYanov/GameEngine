use super::{InputFrame, SimulationSession};
use crate::{
	EntityKey,
	animation::AnimationBank,
	runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle},
	scene_audio::{AudioOnsetCause, CommittedAudioOnset, SceneAudioVoice, available_onsets},
	scene_catalog::SceneId,
	sequence::{SequenceAction, SequenceEvent, SequenceInstance, SequenceLibrary, SequenceTrigger},
	triggers::{TriggerEvent, TriggerPhase},
	world::RuntimeWorld,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const SKIP_ACTION: &str = "skip_sequence";

#[derive(Clone, Default)]
pub(super) struct SequenceDirector {
	queued: Option<EntityKey>,
	active: Option<Playback>,
	completed: BTreeSet<EntityKey>,
	skip: bool,
	cancel: bool,
}

#[derive(Clone)]
struct Playback {
	instance: EntityKey,
	role_owners: BTreeMap<EntityKey, RuntimeEntityHandle>,
	actors: BTreeSet<EntityKey>,
	clips: BTreeMap<EntityKey, (u32, u32)>,
	camera: Option<RuntimeEntityHandle>,
	tick_started: u64,
	next_cue: usize,
	elapsed_ms: u32,
	subtitle: Option<(String, u32)>,
}

pub(super) struct SequenceStep<'a> {
	pub(super) library: &'a SequenceLibrary,
	pub(super) scene: &'a SceneId,
	pub(super) world: &'a RuntimeWorld,
	pub(super) lifecycle: &'a RuntimeLifecycle,
	pub(super) triggers: &'a [TriggerEvent],
	pub(super) tick: u64,
	pub(super) fixed_hz: u32,
	pub(super) skip_pressed: bool,
}

impl SequenceDirector {
	pub(super) fn needs_tick(&self) -> bool {
		self.active.is_some() || self.queued.is_some()
	}

	pub(super) fn camera(&self) -> Option<&RuntimeEntityHandle> {
		self.active.as_ref()?.camera.as_ref()
	}

	pub(super) fn owns_actor(&self, key: &EntityKey) -> bool {
		self.active
			.as_ref()
			.is_some_and(|playback| playback.actors.contains(key))
	}

	pub(super) fn sampled_clips(&self) -> impl Iterator<Item=(&EntityKey, u32, f64)> {
		self.active.iter().flat_map(|playback| {
			playback.clips.iter().map(|(key, &(clip, start_ms))| {
				(
					key,
					clip,
					f64::from(playback.elapsed_ms - start_ms) / 1_000.,
				)
			})
		})
	}

	pub(super) fn step(
		&mut self,
		step: SequenceStep<'_>,
		animations: &AnimationBank,
		events: &mut Vec<SequenceEvent>,
		onsets: &mut Vec<CommittedAudioOnset>,
	) {
		if self.active.is_none() {
			let queued = self.queued.take();
			let instance = queued
				.as_ref()
				.and_then(|key| step.library.instance_for(step.scene, key))
				.or_else(|| self.triggered(&step));
			if let Some(instance) = instance {
				match Playback::prepare(instance, &step, animations) {
					Ok(playback) => {
						self.active = Some(playback);
						events.push(SequenceEvent::Started {
							instance: instance.id.clone(),
							tick: step.tick,
						});
					}
					Err(error) => {
						engine_logger::warn!(target: "sequence", "Sequence {} could not start: {error}", instance.id.as_str());
						events.push(SequenceEvent::Cancelled {
							instance: instance.id.clone(),
							tick: step.tick,
						});
					}
				}
			}
		}
		let skip = std::mem::take(&mut self.skip) || step.skip_pressed;
		let cancel = std::mem::take(&mut self.cancel);
		let Some(playback) = &mut self.active else {
			return;
		};
		if cancel || !playback.bindings_are_live(step.world, step.lifecycle) {
			self.cancel_active(step.tick, events, onsets);
			return;
		}
		let instance = step
			.library
			.instance_for(step.scene, &playback.instance)
			.expect("active sequence has a pinned instance");
		let asset = step
			.library
			.asset(&instance.sequence_id)
			.expect("active sequence has a pinned asset");
		let elapsed_ms = (step.tick - playback.tick_started) * 1_000 / u64::from(step.fixed_hz);
		if skip {
			let instance = playback.instance.clone();
			self.completed.insert(instance.clone());
			self.active = None;
			events.push(SequenceEvent::Skipped {
				instance,
				tick: step.tick,
			});
			return;
		}
		playback.elapsed_ms = elapsed_ms.min(u64::from(asset.duration_ms)) as u32;
		while let Some(cue) = asset
			.cues
			.get(playback.next_cue)
			.filter(|cue| cue.at_ms <= playback.elapsed_ms)
		{
			match &cue.action {
				SequenceAction::CameraCut { role } => {
					playback.camera = Some(playback.role_owners[role].clone());
				}
				SequenceAction::PlayClip { role, clip_index } => {
					playback.clips.insert(
						playback.role_owners[role].key.clone(),
						(*clip_index, cue.at_ms),
					);
				}
				SequenceAction::Audio { role } => {
					record_sequence_audio(
						step.world,
						&playback.role_owners[role].key,
						&playback.instance,
						playback.next_cue,
						step.tick,
						onsets,
					);
				}
				SequenceAction::Subtitle { text, duration_ms } => {
					playback.subtitle = Some((text.clone(), cue.at_ms.saturating_add(*duration_ms)));
					events.push(SequenceEvent::Subtitle {
						instance: playback.instance.clone(),
						tick: step.tick,
						text: text.clone(),
						duration_ms: *duration_ms,
					});
				}
			}
			playback.next_cue += 1;
		}
		if elapsed_ms >= u64::from(asset.duration_ms) {
			let instance = playback.instance.clone();
			self.completed.insert(instance.clone());
			self.active = None;
			events.push(SequenceEvent::Finished {
				instance,
				tick: step.tick,
			});
		}
	}

	pub(super) fn abort_stale(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		tick: u64,
		events: &mut Vec<SequenceEvent>,
		onsets: &mut Vec<CommittedAudioOnset>,
	) {
		if self
			.active
			.as_ref()
			.is_some_and(|playback| !playback.bindings_are_live(world, lifecycle))
		{
			self.cancel_active(tick, events, onsets);
		}
	}

	fn cancel_active(
		&mut self,
		tick: u64,
		events: &mut Vec<SequenceEvent>,
		onsets: &mut Vec<CommittedAudioOnset>,
	) {
		let Some(playback) = self.active.take() else {
			return;
		};
		let instance = playback.instance;
		events.retain(|event| {
			!matches!(
				event,
				SequenceEvent::Subtitle { instance: owner, tick: at, .. } if owner == &instance && *at == tick
			)
		});
		onsets.retain(|onset| {
			!matches!(
				&onset.cause,
				AudioOnsetCause::Sequence { instance: owner, .. } if owner == &instance && onset.tick == tick
			)
		});
		events.push(SequenceEvent::Cancelled { instance, tick });
	}

	fn triggered<'a>(&self, step: &SequenceStep<'a>) -> Option<&'a SequenceInstance> {
		step.library
			.instances
			.iter()
			.filter(|instance| &instance.scene_id == step.scene)
			.filter(|instance| !instance.once_per_entry || !self.completed.contains(&instance.id))
			.filter(|instance| {
				instance.trigger.as_ref().is_some_and(|trigger| {
					step.triggers
						.iter()
						.any(|event| sensor_enter(event, trigger))
				})
			})
			.min_by(|left, right| left.id.cmp(&right.id))
	}

	pub(super) fn replay_state(&self) -> serde_json::Value {
		serde_json::json!({
			"queued": self.queued,
			"skip": self.skip,
			"cancel": self.cancel,
			"completed": self.completed,
			"active": self.active.as_ref().map(|playback| serde_json::json!({
				"instance": playback.instance,
				"started": playback.tick_started,
				"next_cue": playback.next_cue,
				"elapsed_ms": playback.elapsed_ms,
				"clips": playback.clips,
				"camera": playback.camera.as_ref().map(|camera| &camera.key),
			})),
		})
	}
}

impl Playback {
	fn prepare(
		instance: &SequenceInstance,
		step: &SequenceStep<'_>,
		animations: &AnimationBank,
	) -> Result<Self, String> {
		let asset = step
			.library
			.asset(&instance.sequence_id)
			.ok_or("Sequence asset is missing")?;
		let role_owners = instance
			.bindings
			.iter()
			.map(|(role, key)| {
				step.lifecycle
					.handle(key)
					.map(|handle| (role.clone(), handle))
					.ok_or_else(|| {
						format!("Sequence role {} is no longer in the scene", role.as_str())
					})
			})
			.collect::<Result<BTreeMap<_, _>, _>>()?;
		let mut actors = BTreeSet::new();
		for cue in &asset.cues {
			let key = |role| {
				role_owners
					.get(role)
					.map(|handle| &handle.key)
					.ok_or("Sequence role has no binding")
			};
			match &cue.action {
				SequenceAction::PlayClip { role, clip_index } => {
					let actor = key(role)?;
					animations.require_sequence_clip(actor, *clip_index)?;
					actors.insert(actor.clone());
				}
				SequenceAction::CameraCut { role } => {
					let owner = role_owners.get(role).ok_or("Camera role has no binding")?;
					super::camera_objects::CameraObjects::projection(
						owner,
						step.world,
						step.lifecycle,
					)
						.ok_or("Sequence camera is unavailable")?;
				}
				SequenceAction::Audio { role } => {
					let actor = key(role)?;
					let index = step
						.world
						.entity_index(actor)
						.ok_or("Sequence audio entity is missing")?;
					step.world.data().entities[index]
						.audio_source
						.as_ref()
						.filter(|source| source.enabled && source.settings.clip.is_some())
						.ok_or("Sequence audio source is unavailable")?;
				}
				SequenceAction::Subtitle { .. } => {}
			}
		}
		Ok(Self {
			instance: instance.id.clone(),
			role_owners,
			actors,
			clips: BTreeMap::new(),
			camera: None,
			tick_started: step.tick,
			next_cue: 0,
			elapsed_ms: 0,
			subtitle: None,
		})
	}

	fn bindings_are_live(&self, world: &RuntimeWorld, lifecycle: &RuntimeLifecycle) -> bool {
		self.role_owners
			.values()
			.all(|handle| lifecycle.validate_handle(handle).is_ok())
			&& self.camera.as_ref().is_none_or(|camera| {
			super::camera_objects::CameraObjects::projection(camera, world, lifecycle).is_some()
		})
	}
}

impl SimulationSession {
	pub fn active_sequence(&self) -> Option<&EntityKey> {
		self.sequences
			.active
			.as_ref()
			.map(|playback| &playback.instance)
	}

	pub fn sequence_presentation(&self) -> Option<crate::sequence::SequencePresentation> {
		let playback = self.sequences.active.as_ref()?;
		Some(crate::sequence::SequencePresentation {
			instance: playback.instance.clone(),
			subtitle: playback.subtitle.as_ref().filter(|(_, end)| playback.elapsed_ms < *end)
				.map(|(text, _)| text.clone()),
		})
	}

	pub fn queue_sequence(&mut self, id: &EntityKey) -> Result<(), String> {
		let instance = self
			.project
			.sequences
			.instance_for(&self.scene_id, id)
			.ok_or("Sequence instance is not registered in the active scene")?;
		if self.sequences.needs_tick() {
			return Err("A sequence is already active or queued".into());
		}
		if instance.once_per_entry && self.sequences.completed.contains(id) {
			return Err("Sequence has already completed in this scene entry".into());
		}
		self.sequences.queued = Some(id.clone());
		Ok(())
	}

	pub fn skip_sequence(&mut self) -> bool {
		let active = self.sequences.needs_tick();
		self.sequences.skip |= active;
		active
	}

	pub fn cancel_sequence(&mut self) -> bool {
		let active = self.sequences.needs_tick();
		self.sequences.cancel |= active;
		active
	}
}

fn sensor_enter(event: &TriggerEvent, trigger: &SequenceTrigger) -> bool {
	event.phase == TriggerPhase::Enter
		&& event.sensor == trigger.sensor
		&& event.other == trigger.actor
}

pub(super) fn neutral_gameplay_input(input: &InputFrame) -> InputFrame {
	InputFrame {
		local_player: input.local_player.clone(),
		controller_owned: input.controller_owned,
		possession_managed: input.possession_managed,
		..InputFrame::default()
	}
}

fn record_sequence_audio(
	world: &RuntimeWorld,
	key: &EntityKey,
	instance: &EntityKey,
	cue: usize,
	tick: u64,
	onsets: &mut Vec<CommittedAudioOnset>,
) {
	if available_onsets(onsets, tick) == 0 {
		return;
	}
	let Some(index) = world.entity_index(key) else {
		return;
	};
	let Some(source) = world.data().entities[index]
		.audio_source
		.as_ref()
		.filter(|source| source.enabled)
	else {
		return;
	};
	let Some(clip) = source.settings.clip.as_ref() else {
		return;
	};
	onsets.push(CommittedAudioOnset {
		voice: SceneAudioVoice {
			entity: key.clone(),
			component: source.id.clone(),
			clip: clip.asset.clone(),
			gain: source.settings.gain,
			looping: false,
			bus: source.settings.bus,
			spatial: source.settings.spatial,
		},
		tick,
		cause: AudioOnsetCause::Sequence {
			instance: instance.clone(),
			cue: cue as u32,
		},
		replaces_cue: None,
	});
}
