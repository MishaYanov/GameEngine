use super::*;
use crate::{
	EntityKey, components::ComponentId, runtime_lifecycle::RuntimeEntityHandle, world::RuntimeWorld,
};
use glam::{Quat, Vec3};
use serde::Serialize;
use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	sync::Arc,
};

pub type AnimatorResolver = Arc<
	dyn Fn(
			&crate::project::ProjectData,
			&crate::scene_catalog::SceneId,
			&crate::SceneData,
		) -> Result<PreparedAnimatorBindings, String>
		+ Send
		+ Sync,
>;
pub type PreparedAnimatorBindings = BTreeMap<EntityKey, Arc<PreparedAnimator>>;

pub struct PreparedAnimator {
	pub graph: Arc<AnimatorGraph>,
	pub clips: PreparedAnimatorClips,
}

pub enum PreparedAnimatorClips {
	Imported(Arc<AnimationAsset>),
	Visual(Vec<ResolvedAuthoredClip>),
}

pub struct AnimatorStepInputs<'a> {
	pub motion: Option<&'a AnimationMotion>,
	pub jumped: bool,
	pub accepted_actions: &'a HashSet<String>,
	pub cosmetic: Option<&'a BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnimatorInspection {
	pub entity: RuntimeEntityHandle,
	pub component_id: ComponentId,
	pub state_id: String,
	pub state_ticks: u64,
	pub transition: Option<String>,
	pub blend_age: Option<u16>,
	pub blend_ticks: Option<u16>,
}

#[derive(Clone)]
struct AnimatorMachine {
	index: usize,
	state_ticks: u64,
	transition: Option<usize>,
	blend_clock: Option<(u16, u16)>,
}

#[derive(Clone)]
enum Pose {
	Skeletal(AnimationPose),
	Visual(Vec<VisualPose>),
}

#[derive(Clone)]
struct AnimatorInstance {
	owner: RuntimeEntityHandle,
	component_id: ComponentId,
	prepared: Arc<PreparedAnimator>,
	machine: AnimatorMachine,
	pose: Pose,
	fade_start: Option<Pose>,
}

#[derive(Clone, Default)]
pub(crate) struct AnimatorBank {
	instances: BTreeMap<EntityKey, AnimatorInstance>,
}

impl AnimatorMachine {
	fn new(graph: &AnimatorGraph) -> Result<Self, String> {
		graph.validate()?;
		let index = graph
			.states
			.iter()
			.position(|state| state.id == graph.entry)
			.ok_or("Animator entry state is missing")?;
		Ok(Self {
			index,
			state_ticks: 0,
			transition: None,
			blend_clock: None,
		})
	}

	/// The first eligible edge in authored order wins. Triggers are sampled only this step.
	fn advance(
		&mut self,
		graph: &AnimatorGraph,
		input: &AnimatorStepInputs<'_>,
	) -> Result<bool, String> {
		self.transition = None;
		if let Some((age, duration)) = &mut self.blend_clock {
			*age = age.saturating_add(1);
			if *age >= *duration {
				self.blend_clock = None;
			}
		} else {
			let previous = &graph.states[self.index].id;
			if let Some((index, edge)) = graph.transitions.iter().enumerate().find(|(_, edge)| {
				&edge.from == previous
					&& edge
						.exit_after_ticks
						.is_none_or(|ticks| self.state_ticks >= u64::from(ticks))
					&& condition_matches(graph, &edge.condition, input)
			}) {
				self.index = graph
					.states
					.iter()
					.position(|state| state.id == edge.to)
					.ok_or("Animator transition destination is missing")?;
				self.state_ticks = 0;
				self.transition = Some(index);
				self.blend_clock = (edge.blend_ticks > 0).then_some((0, edge.blend_ticks));
				return Ok(true);
			}
		}
		self.state_ticks = self
			.state_ticks
			.checked_add(1)
			.filter(|ticks| *ticks <= 1_000_000_000_000)
			.ok_or("Animator clock limit exceeded")?;
		Ok(false)
	}
}

fn condition_matches(
	graph: &AnimatorGraph,
	condition: &AnimatorCondition,
	input: &AnimatorStepInputs<'_>,
) -> bool {
	let parameter = match condition {
		AnimatorCondition::Always => return true,
		AnimatorCondition::Bool { parameter, .. } | AnimatorCondition::Trigger { parameter } => {
			parameter
		}
	};
	let Some(definition) = graph.parameters.iter().find(|entry| entry.id == *parameter) else {
		return false;
	};
	let value = match &definition.source {
		AnimatorParameterSource::Boolean { default } => input
			.cosmetic
			.and_then(|values| values.get(parameter))
			.copied()
			.unwrap_or(*default),
		AnimatorParameterSource::Grounded => input.motion.is_some_and(|motion| motion.grounded),
		AnimatorParameterSource::Moving {
			minimum_mm_per_second,
		} => input.motion.is_some_and(|motion| {
			motion.planar_speed * 1000.0 >= f32::from(*minimum_mm_per_second)
		}),
		AnimatorParameterSource::Jumped => input.jumped,
		AnimatorParameterSource::ActionAccepted { action } => {
			input.accepted_actions.contains(action)
		}
	};
	match condition {
		AnimatorCondition::Bool {
			value: expected, ..
		} => value == *expected,
		_ => value,
	}
}

impl AnimatorBank {
	pub(crate) fn new(
		world: &RuntimeWorld,
		bindings: &PreparedAnimatorBindings,
		handles: &BTreeMap<EntityKey, RuntimeEntityHandle>,
		fixed_hz: u32,
	) -> Result<Self, String> {
		let mut present = BTreeSet::new();
		let mut instances = BTreeMap::new();
		for entity in &world.data().entities {
			let Some(component) = entity
				.animator
				.as_ref()
				.filter(|component| component.enabled)
			else {
				continue;
			};
			let prepared = Arc::clone(bindings.get(&entity.key).ok_or_else(|| {
				format!("Animator on {:?} was not prepared", entity.key.as_str())
			})?);
			prepared
				.graph
				.validate_binding(&component.settings.binding)?;
			if instances.len() >= MAX_ANIMATION_INSTANCES {
				return Err("World exceeds 64 Animator instances".into());
			}
			if let PreparedAnimatorClips::Visual(clips) = &prepared.clips {
				let mut clip_targets = BTreeSet::new();
				for clip in clips {
					clip_targets.extend(clip.target_keys());
				}
				if clip_targets
					.iter()
					.any(|key| !present.insert((*key).clone()))
				{
					return Err("Only one Animator can own a visual target".into());
				}
			}
			let owner = handles
				.get(&entity.key)
				.ok_or("Animator owner has no runtime handle")?
				.clone();
			let machine = AnimatorMachine::new(&prepared.graph)?;
			let pose = sample(world, &owner.key, &prepared, &machine, fixed_hz, 0)?;
			instances.insert(
				entity.key.clone(),
				AnimatorInstance {
					owner,
					component_id: component.id.clone(),
					prepared,
					machine,
					pose,
					fade_start: None,
				},
			);
		}
		Ok(Self { instances })
	}

	pub(crate) fn preserve_existing(&mut self, previous: &Self) {
		for (key, instance) in &mut self.instances {
			if let Some(old) = previous.instances.get(key)
				&& instance.owner == old.owner
				&& instance.component_id == old.component_id
			{
				*instance = old.clone();
			}
		}
	}

	pub(crate) fn prune(&mut self, world: &RuntimeWorld) {
		self.instances
			.retain(|key, _| world.entity_index(key).is_some());
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.instances.is_empty()
	}

	pub(crate) fn poses(&self) -> impl Iterator<Item = (&EntityKey, &AnimationPose)> {
		self.instances
			.iter()
			.filter_map(|(key, instance)| match &instance.pose {
				Pose::Skeletal(pose) => Some((key, pose)),
				Pose::Visual(_) => None,
			})
	}

	pub(crate) fn visual_poses(&self) -> impl Iterator<Item = &VisualPose> {
		self.instances
			.values()
			.flat_map(|instance| match &instance.pose {
				Pose::Visual(poses) => poses.iter(),
				Pose::Skeletal(_) => [].iter(),
			})
	}

	pub(crate) fn visual_matrices(
		&self,
		world: &RuntimeWorld,
	) -> Result<BTreeMap<EntityKey, glam::Mat4>, String> {
		let locals = self
			.visual_poses()
			.map(|pose| {
				(
					&pose.entity_key,
					glam::Mat4::from_scale_rotation_translation(
						Vec3::from_array(pose.scale),
						Quat::from_array(pose.rotation_xyzw),
						Vec3::from_array(pose.position),
					),
				)
			})
			.collect::<BTreeMap<_, _>>();
		let mut result = BTreeMap::new();
		for key in locals.keys() {
			let root = world
				.entity_index(key)
				.ok_or("Animator visual target is absent")?;
			let mut ancestor = world.data().entities[root].parent.as_ref();
			while let Some(parent) = ancestor {
				if locals.contains_key(parent) {
					break;
				}
				ancestor = world
					.entity_index(parent)
					.and_then(|index| world.data().entities[index].parent.as_ref());
			}
			if ancestor.is_some() {
				continue;
			}
			for index in world.subtree_indices(root)? {
				let entity = &world.data().entities[index];
				let parent = entity
					.parent
					.as_ref()
					.and_then(|key| result.get(key))
					.copied();
				let parent = parent.unwrap_or(world.parent_matrix(index)?);
				let local = locals
					.get(&entity.key)
					.copied()
					.unwrap_or_else(|| entity.transform.matrix());
				let world_matrix = parent * local;
				if !world_matrix.is_finite() {
					return Err("Animator visual placement is not finite".into());
				}
				result.insert(entity.key.clone(), world_matrix);
			}
		}
		Ok(result)
	}

	pub(crate) fn inspection(&self) -> Vec<AnimatorInspection> {
		self.instances
			.values()
			.map(|instance| AnimatorInspection {
				entity: instance.owner.clone(),
				component_id: instance.component_id.clone(),
				state_id: instance.prepared.graph.states[instance.machine.index]
					.id
					.clone(),
				state_ticks: instance.machine.state_ticks,
				transition: instance
					.machine
					.transition
					.map(|index| instance.prepared.graph.transitions[index].id.clone()),
				blend_age: instance.machine.blend_clock.map(|clock| clock.0),
				blend_ticks: instance.machine.blend_clock.map(|clock| clock.1),
			})
			.collect()
	}

	pub(crate) fn step(
		&mut self,
		world: &RuntimeWorld,
		motions: &BTreeMap<EntityKey, AnimationMotion>,
		jumped: &HashSet<EntityKey>,
		local_player: Option<&EntityKey>,
		accepted_actions: &HashSet<String>,
		cosmetic: &BTreeMap<String, bool>,
		fixed_hz: u32,
		revision: u64,
	) -> Result<Vec<EntityKey>, String> {
		let no_actions = HashSet::new();
		let mut changed = BTreeSet::new();
		for (key, instance) in &mut self.instances {
			if world.entity_index(key).is_none() {
				continue;
			}
			let local = local_player == Some(key);
			let input = AnimatorStepInputs {
				motion: motions
					.get(key)
					.filter(|motion| motion.entity == instance.owner),
				jumped: jumped.contains(key),
				accepted_actions: if local { accepted_actions } else { &no_actions },
				cosmetic: local.then_some(cosmetic),
			};
			let transition = instance.machine.advance(&instance.prepared.graph, &input)?;
			if transition && instance.machine.blend_clock.is_some() {
				instance.fade_start = Some(instance.pose.clone());
			}
			let mut pose = sample(
				world,
				key,
				&instance.prepared,
				&instance.machine,
				fixed_hz,
				revision,
			)?;
			if let Some((age, duration)) = instance.machine.blend_clock
				&& let Some(from) = &instance.fade_start
			{
				blend(
					world,
					key,
					from,
					&mut pose,
					f32::from(age) / f32::from(duration),
				)?;
			} else {
				instance.fade_start = None;
			}
			match (&instance.pose, &pose) {
				(Pose::Visual(before), Pose::Visual(after)) if before != after => {
					changed.extend(
						before
							.iter()
							.chain(after)
							.map(|pose| pose.entity_key.clone()),
					);
				}
				(Pose::Skeletal(before), Pose::Skeletal(after))
					if before.local != after.local || before.clip_index != after.clip_index =>
				{
					changed.insert(key.clone());
				}
				_ => {}
			}
			instance.pose = pose;
		}
		self.prune(world);
		Ok(changed.into_iter().collect())
	}
}
fn sample(
	world: &RuntimeWorld,
	key: &EntityKey,
	prepared: &PreparedAnimator,
	machine: &AnimatorMachine,
	fixed_hz: u32,
	revision: u64,
) -> Result<Pose, String> {
	if fixed_hz == 0 {
		return Err("Animator fixed rate cannot be zero".into());
	}
	let state = &prepared.graph.states[machine.index];
	match (&prepared.clips, &state.clip) {
		(PreparedAnimatorClips::Imported(asset), AnimationClipRef::Imported { clip_index, .. }) => {
			let duration = asset
				.clips
				.get(*clip_index as usize)
				.ok_or("Animator clip disappeared")?
				.duration_seconds;
			let seconds = machine.state_ticks as f64 / f64::from(fixed_hz);
			let local = sample_local(
				asset,
				*clip_index as usize,
				clip_time(seconds, duration, state.looping)?,
			)?;
			let placement = world.world_matrices()[world
				.entity_index(key)
				.ok_or("Animator object is missing")?];
			instance::make_pose(
				Arc::clone(asset),
				local,
				placement,
				machine.state_ticks,
				*clip_index,
				MotionMode::Idle,
				revision,
			)
			.map(Pose::Skeletal)
		}
		(PreparedAnimatorClips::Visual(clips), AnimationClipRef::Authored { .. }) => {
			let clip = clips
				.get(machine.index)
				.ok_or("Animator visual clip was not prepared")?;
			let duration = u64::from(clip.duration_millis());
			let elapsed = machine.state_ticks.saturating_mul(1000) / u64::from(fixed_hz);
			let time = if state.looping {
				elapsed % duration
			} else {
				elapsed.min(duration)
			};
			clip.sample(time as u32).map(Pose::Visual)
		}
		_ => Err("Animator runtime clip type differs from the validated binding".into()),
	}
}

fn blend(
	world: &RuntimeWorld,
	key: &EntityKey,
	from: &Pose,
	result: &mut Pose,
	weight: f32,
) -> Result<(), String> {
	match (from, result) {
		(Pose::Skeletal(before), Pose::Skeletal(next)) => {
			let local = blend_poses(&before.local, &next.local, weight)?;
			let placement = world.world_matrices()[world
				.entity_index(key)
				.ok_or("Animator object is missing")?];
			*next = instance::make_pose(
				Arc::clone(&next.asset),
				local,
				placement,
				next.elapsed_ticks,
				next.clip_index,
				next.mode,
				next.revision,
			)?;
		}
		(Pose::Visual(before), Pose::Visual(next)) => {
			let before: BTreeMap<_, _> =
				before.iter().map(|pose| (&pose.entity_key, pose)).collect();
			let current: BTreeMap<_, _> =
				next.iter().map(|pose| (&pose.entity_key, pose)).collect();
			let keys = before
				.keys()
				.chain(current.keys())
				.copied()
				.collect::<BTreeSet<_>>();
			let mut blended = Vec::with_capacity(keys.len());
			for key in keys {
				let entity = &world.data().entities[world
					.entity_index(key)
					.ok_or("Animator visual target is missing")?];
				let [x, y, z] = entity.transform.rotation;
				let rest = VisualPose {
					entity_key: key.clone(),
					position: entity.transform.position,
					scale: entity.transform.scale,
					rotation_xyzw: Quat::from_euler(glam::EulerRot::XYZ, x, y, z).to_array(),
				};
				let previous = before.get(key).copied().unwrap_or(&rest);
				let target = current.get(key).copied().unwrap_or(&rest);
				blended.push(VisualPose {
					entity_key: key.clone(),
					position: Vec3::from_array(previous.position)
						.lerp(Vec3::from_array(target.position), weight)
						.to_array(),
					scale: Vec3::from_array(previous.scale)
						.lerp(Vec3::from_array(target.scale), weight)
						.to_array(),
					rotation_xyzw: Quat::from_array(previous.rotation_xyzw)
						.slerp(Quat::from_array(target.rotation_xyzw), weight)
						.normalize()
						.to_array(),
				});
			}
			*next = blended;
		}
		_ => return Err("Animator cannot crossfade different clip kinds".into()),
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::asset_catalog::AssetId;

	#[test]
	fn graph_transition_priority_and_trigger_expiration_are_per_actor() {
		let clip = AnimationClipRef::Authored {
			asset_id: AssetId::generate(),
			expected_sha256: "a".repeat(64),
		};
		let state = |id: &str| AnimatorState {
			id: id.into(),
			name: id.into(),
			clip: clip.clone(),
			looping: false,
		};
		let edge = |id: &str, from: &str, to: &str, blend_ticks| AnimatorTransition {
			id: id.into(),
			from: from.into(),
			to: to.into(),
			condition: AnimatorCondition::Trigger {
				parameter: "takeoff".into(),
			},
			exit_after_ticks: None,
			blend_ticks,
		};
		let graph = AnimatorGraph {
			schema_version: 1,
			entry: "idle".into(),
			states: vec![state("idle"), state("jump"), state("other")],
			parameters: vec![AnimatorParameter {
				id: "takeoff".into(),
				name: "Takeoff".into(),
				source: AnimatorParameterSource::Jumped,
			}],
			transitions: vec![
				edge("primary", "idle", "jump", 2),
				edge("secondary", "idle", "other", 0),
				edge("return", "jump", "idle", 0),
			],
			layout: Default::default(),
		};
		let no_actions = HashSet::new();
		let input = |jumped| AnimatorStepInputs {
			motion: None,
			jumped,
			accepted_actions: &no_actions,
			cosmetic: None,
		};
		let mut first = AnimatorMachine::new(&graph).unwrap();
		let mut second = AnimatorMachine::new(&graph).unwrap();
		assert!(first.advance(&graph, &input(true)).unwrap());
		assert!(!second.advance(&graph, &input(false)).unwrap());
		assert_eq!(first.transition, Some(0));
		assert_eq!(first.index, 1);
		assert_eq!(second.index, 0);
		assert_eq!(first.blend_clock, Some((0, 2)));
		assert!(!first.advance(&graph, &input(true)).unwrap());
		assert!(!first.advance(&graph, &input(false)).unwrap());
		assert_eq!(first.blend_clock, None);
		assert!(!first.advance(&graph, &input(false)).unwrap());
		assert_eq!(first.index, 1);
		assert!(first.advance(&graph, &input(true)).unwrap());
		assert_eq!(first.index, 0);
	}
}
