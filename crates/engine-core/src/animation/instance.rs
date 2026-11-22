use super::*;
use crate::{EntityKey, runtime_lifecycle::RuntimeEntityHandle, world::RuntimeWorld};
use glam::Mat4;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

pub type PreparedAnimationBindings = BTreeMap<EntityKey, Arc<AnimationAsset>>;
pub type AnimationResolver =
	Arc<dyn Fn(&crate::SceneData) -> Result<PreparedAnimationBindings, String> + Send + Sync>;
#[derive(Clone, Debug)]
pub struct AnimationMotion {
	pub entity: RuntimeEntityHandle,
	pub tick: u64,
	pub grounded: bool,
	pub planar_speed: f32,
	pub vertical_speed: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
	Idle,
	Walk,
	Air,
}
#[derive(Clone, Debug)]
pub struct AnimationPose {
	pub asset: Arc<AnimationAsset>,
	pub local: Vec<LocalTrs>,
	pub globals: Vec<Mat4>,
	pub palette: Vec<Mat4>,
	pub mesh_matrix: Mat4,
	pub bounds: crate::Bounds3,
	pub elapsed_ticks: u64,
	pub clip_index: u32,
	pub mode: MotionMode,
	pub revision: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct AnimationInspection {
	pub entity: RuntimeEntityHandle,
	pub component_id: crate::components::ComponentId,
	pub clip_index: u32,
	pub elapsed_ticks: u64,
	pub mode: MotionMode,
	pub playing: bool,
	pub blending: bool,
}
#[derive(Clone)]
struct Instance {
	owner: RuntimeEntityHandle,
	component: AnimationComponent,
	runtime_playback: Option<(u32, bool)>,
	pose: AnimationPose,
	fade: Option<(Vec<LocalTrs>, u16)>,
	active: bool,
}
#[derive(Clone, Default)]
pub struct AnimationBank {
	instances: BTreeMap<EntityKey, Instance>,
	revision: u64,
}

impl AnimationBank {
	pub fn new(
		world: &RuntimeWorld,
		bindings: &PreparedAnimationBindings,
		handles: &BTreeMap<EntityKey, RuntimeEntityHandle>,
	) -> Result<Self, String> {
		let mut bank = Self::default();
		let mut unique = std::collections::HashSet::new();
		let mut bytes = 0usize;
		for asset in bindings.values() {
			if unique.insert(Arc::as_ptr(asset) as usize) {
				bytes = bytes
					.checked_add(asset.retained_bytes())
					.ok_or("World animation byte overflow")?;
				if bytes > 128 * 1024 * 1024 {
					return Err("World animation CPU assets exceed 128 MiB".into());
				}
			}
		}
		for (index, entity) in world.data().entities.iter().enumerate() {
			let Some(component) = entity.animation.as_ref() else {
				continue;
			};
			if component.enabled
				&& matches!(
					component.settings.playback,
					AnimationPlayback::Locomotion3 { .. }
				) && entity
				.character_controller
				.as_ref()
				.is_none_or(|c| !c.enabled)
			{
				return Err("Idle/Walk/Air animation must share its entity with an enabled character controller".into());
			}
			if bank.instances.len() >= MAX_ANIMATION_INSTANCES {
				return Err("World exceeds 64 animated instances".into());
			}
			let asset = Arc::clone(bindings.get(&entity.key).ok_or_else(|| {
				format!("Animation asset not prepared for {}", entity.key.as_str())
			})?);
			asset.validate_settings(&component.settings)?;
			let clip_index = match component.settings.playback {
				AnimationPlayback::SingleClip { clip_index, .. } => clip_index,
				AnimationPlayback::Locomotion3 { idle, .. } => idle,
			};
			let local = if component.enabled {
				sample_local(&asset, clip_index as usize, 0.)?
			} else {
				asset.nodes.iter().map(|n| n.rest).collect()
			};
			let pose = make_pose(
				asset,
				local,
				world.world_matrices()[index],
				0,
				clip_index,
				MotionMode::Idle,
				0,
			)?;
			let active =
				component.enabled && pose.asset.clips[clip_index as usize].duration_seconds > 0.;
			bank.instances.insert(
				entity.key.clone(),
				Instance {
					owner: handles
						.get(&entity.key)
						.ok_or("Animation owner has no runtime handle")?
						.clone(),
					component: component.clone(),
					runtime_playback: None,
					pose,
					fade: None,
					active,
				},
			);
		}
		Ok(bank)
	}
	pub fn inspection(&self, paused: bool) -> Vec<AnimationInspection> {
		self.instances
			.values()
			.map(|i| AnimationInspection {
				entity: i.owner.clone(),
				component_id: i.component.id.clone(),
				clip_index: i.pose.clip_index,
				elapsed_ticks: i.pose.elapsed_ticks,
				mode: i.pose.mode,
				playing: i.active && !paused,
				blending: i.fade.is_some(),
			})
			.collect()
	}
	pub fn replay_state(&self) -> serde_json::Value {
		serde_json::Value::Array(self.instances.iter().map(|(key, i)| {
			let mut state = serde_json::json!({
            "key":key,"generation":i.owner.generation,"source_sha256":i.pose.asset.source_sha256,
            "component_id":i.component.id,"enabled":i.component.enabled,"settings":i.component.settings,
            "elapsed_ticks":i.pose.elapsed_ticks,"clip_index":i.pose.clip_index,"mode":i.pose.mode,"active":i.active,
            "local":i.pose.local,"globals":i.pose.globals.iter().map(|m|m.to_cols_array()).collect::<Vec<_>>(),
            "palette":i.pose.palette.iter().map(|m|m.to_cols_array()).collect::<Vec<_>>(),
            "bounds":{"min":i.pose.bounds.min(),"max":i.pose.bounds.max()},
            "fade":i.fade.as_ref().map(|(pose,age)|serde_json::json!({"age":age,"outgoing_pose":pose}))
			});
			if let Some((clip_index, looping)) = i.runtime_playback {
				state["runtime_playback"] = serde_json::json!({ "clip_index": clip_index, "looping": looping });
			}
			state
		}).collect())
	}
	pub fn needs_tick(&self) -> bool {
		self.instances
			.values()
			.any(|i| i.active || i.fade.is_some())
	}
	pub fn has_active_runtime_one_shot(&self) -> bool {
		self.instances.values().any(|instance| {
			instance.active && instance.runtime_playback.is_some_and(|(_, looping)| !looping)
		})
	}
	pub fn is_empty(&self) -> bool {
		self.instances.is_empty()
	}
	pub fn revision(&self) -> u64 {
		self.revision
	}
	pub fn poses(&self) -> impl Iterator<Item = (&EntityKey, &AnimationPose)> {
		self.instances.iter().map(|(k, i)| (k, &i.pose))
	}
	pub fn pose(&self, key: &EntityKey) -> Option<&AnimationPose> {
		self.instances.get(key).map(|i| &i.pose)
	}
	pub fn prune(&mut self, world: &RuntimeWorld) {
		self.instances
			.retain(|k, _| world.entity_index(k).is_some());
	}
	pub fn preserve_existing(&mut self, previous: &Self) {
		for (key, instance) in &mut self.instances {
			if let Some(old) = previous.instances.get(key)
				&& old.owner == instance.owner
				&& old.component == instance.component
			{
				*instance = old.clone();
			}
		}
		self.revision = previous.revision;
	}

	pub(crate) fn play_clip(
		&mut self,
		key: &EntityKey,
		clip: u32,
		looping: bool,
	) -> Result<(), String> {
		let instance = self
			.instances
			.get_mut(key)
			.ok_or("Prepare the animation before selecting a runtime clip")?;
		if !instance.component.enabled
			|| !matches!(
				instance.component.settings.playback,
				AnimationPlayback::SingleClip { .. }
			) {
			return Err("Runtime clip selection requires an enabled single-clip component".into());
		}
		if clip as usize >= instance.pose.asset.clips.len() {
			return Err("Runtime animation clip is absent from the prepared asset".into());
		}
		instance.runtime_playback = Some((clip, looping));
		Ok(())
	}
	/// Stage this bank alongside the tick's world candidate; publish only with aggregate success.
	pub fn step(
		&mut self,
		world: &RuntimeWorld,
		motions: &BTreeMap<EntityKey, AnimationMotion>,
		tick: u64,
		fixed_hz: u32,
	) -> Result<bool, String> {
		if fixed_hz == 0 {
			return Err("Animation fixed rate cannot be zero".into());
		}
		let mut next = self.instances.clone();
		next.retain(|k, _| world.entity_index(k).is_some());
		let mut changed = false;
		let next_revision = self
			.revision
			.checked_add(1)
			.ok_or("Animation revision overflow")?;
		for (key, instance) in &mut next {
			let index = world
				.entity_index(key)
				.ok_or("Animation owner disappeared")?;
			let placement = world.world_matrices()[index];
			if !instance.component.enabled {
				instance.pose = make_pose(
					Arc::clone(&instance.pose.asset),
					instance.pose.local.clone(),
					placement,
					instance.pose.elapsed_ticks,
					instance.pose.clip_index,
					instance.pose.mode,
					instance.pose.revision,
				)?;
				continue;
			}
			let settings = &instance.component.settings;
			let (target, mode, looping) = match settings.playback {
				AnimationPlayback::SingleClip {
					clip_index,
					looping,
				} => {
					let (clip, repeat) = instance.runtime_playback.unwrap_or((clip_index, looping));
					(clip, instance.pose.mode, repeat)
				}
				AnimationPlayback::Locomotion3 { idle, walk, air } => {
					let motion = motions
						.get(key)
						.ok_or("Locomotion animation requires a character controller")?;
					if motion.entity != instance.owner
						|| motion.tick != tick
						|| !motion.planar_speed.is_finite()
						|| motion.planar_speed < 0.
						|| !motion.vertical_speed.is_finite()
					{
						return Err(
							"Animation motion has stale ownership/tick or nonfinite speed".into(),
						);
					}
					let mode = if !motion.grounded {
						MotionMode::Air
					} else if motion.planar_speed
						> if instance.pose.mode == MotionMode::Walk {
						0.05
					} else {
						0.1
					} {
						MotionMode::Walk
					} else {
						MotionMode::Idle
					};
					let target = match mode {
						MotionMode::Idle => idle,
						MotionMode::Walk => walk,
						MotionMode::Air => air,
					};
					(target, mode, true)
				}
			};
			let transition = target != instance.pose.clip_index || mode != instance.pose.mode;
			let duration = instance.pose.asset.clips[target as usize].duration_seconds;
			let elapsed = if transition {
				instance.fade = Some((instance.pose.local.clone(), 0));
				0
			} else if instance.active {
				instance
					.pose
					.elapsed_ticks
					.checked_add(1)
					.filter(|t| *t <= 1_000_000_000_000)
					.ok_or("Animation clock limit exceeded")?
			} else {
				instance.pose.elapsed_ticks
			};
			let seconds = elapsed as f64 * f64::from(settings.speed_numerator)
				/ (f64::from(fixed_hz) * f64::from(settings.speed_denominator));
			let mut local = sample_local(
				&instance.pose.asset,
				target as usize,
				clip_time(seconds, duration, looping)?,
			)?;
			if let Some((from, age)) = &mut instance.fade {
				if !transition {
					*age = age.saturating_add(1).min(6);
				}
				local = blend_poses(from, &local, f32::from(*age) / 6.)?;
				if *age == 6 {
					instance.fade = None;
				}
			}
			instance.active = duration > 0. && (looping || seconds < f64::from(duration));
			changed |= local != instance.pose.local || transition;
			instance.pose = make_pose(
				Arc::clone(&instance.pose.asset),
				local,
				placement,
				elapsed,
				target,
				mode,
				next_revision,
			)?;
		}
		if changed {
			self.revision = self
				.revision
				.checked_add(1)
				.ok_or("Animation revision overflow")?;
		}
		self.instances = next;
		Ok(changed)
	}
}
fn make_pose(
	asset: Arc<AnimationAsset>,
	local: Vec<LocalTrs>,
	placement: Mat4,
	elapsed_ticks: u64,
	clip_index: u32,
	mode: MotionMode,
	revision: u64,
) -> Result<AnimationPose, String> {
	checked_similarity_inverse(placement)?;
	let globals = global_pose(&asset, &local)?;
	for (j, b) in asset.skin.joints.iter().zip(&asset.skin.inverse_bind) {
		checked_similarity_inverse(placement * globals[usize::from(*j)] * *b)?;
	}
	let palette = mesh_local_palette(&asset, &globals)?;
	let mesh_matrix = globals[usize::from(asset.skin.mesh_node)];
	let bounds = animated_bounds(&asset, &globals, placement)?;
	Ok(AnimationPose {
		asset,
		local,
		globals,
		palette,
		mesh_matrix,
		bounds,
		elapsed_ticks,
		clip_index,
		mode,
		revision,
	})
}
pub fn sample_pose(
	asset: Arc<AnimationAsset>,
	clip_index: u32,
	seconds: f64,
	placement: Mat4,
	revision: u64,
) -> Result<AnimationPose, String> {
	let clip = asset
		.clips
		.get(clip_index as usize)
		.ok_or("Unknown animation clip")?;
	if !seconds.is_finite() || seconds < 0. || seconds > f64::from(clip.duration_seconds) {
		return Err("Preview time is outside the clip".into());
	}
	let local = sample_local(&asset, clip_index as usize, seconds)?;
	make_pose(
		asset,
		local,
		placement,
		0,
		clip_index,
		MotionMode::Idle,
		revision,
	)
}
