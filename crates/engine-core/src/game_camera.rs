//! Authored game camera modes and the fixed-step controller that publishes their runtime pose.
use crate::collision::CameraProbeHit;
use crate::runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle};
use crate::{Camera, EntityKey, camera_input::CameraIntent, world::RuntimeWorld};
use glam::{DVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_GAME_ASPECT: f64 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowCameraSettings {
	pub target: EntityKey,
	pub offset: [f32; 3],
	pub look_offset: [f32; 3],
	pub smoothing_seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitCameraSettings {
	pub target: EntityKey,
	pub pivot_offset: [f32; 3],
	pub initial_yaw: f32,
	pub initial_pitch: f32,
	pub initial_distance: f32,
	pub min_distance: f32,
	pub max_distance: f32,
	pub min_pitch: f32,
	pub max_pitch: f32,
	pub follow_seconds: f32,
	pub recovery_seconds: f32,
	pub teleport_distance: f32,
	pub probe_radius: f32,
	pub obstruction_mask: u32,
}

impl FollowCameraSettings {
	pub fn validate(&self) -> Result<(), String> {
		self.target.validate()?;
		if !self
			.offset
			.iter()
			.chain(self.look_offset.iter())
			.all(|value| value.is_finite())
			|| !self.smoothing_seconds.is_finite()
			|| !(0.0..=10.0).contains(&self.smoothing_seconds)
		{
			return Err(
				"Follow camera offsets must be finite and smoothing must be within 0..=10 seconds"
					.into(),
			);
		}
		let direction = Vec3::from_array(self.look_offset) - Vec3::from_array(self.offset);
		let direction = direction
			.try_normalize()
			.ok_or("Follow camera eye and look offsets must define a finite direction")?;
		if direction.y.abs() > 89.0_f32.to_radians().sin() {
			return Err("Follow camera direction must stay within ±89 degrees of pitch".into());
		}
		Ok(())
	}
}

impl OrbitCameraSettings {
	pub fn validate(&self) -> Result<(), String> {
		self.target.validate()?;
		if self.target.as_str().len() > 256 || self.target.as_str().chars().any(char::is_control) {
			return Err("Orbit target must fit 256 bytes without control characters".into());
		}
		if self
			.pivot_offset
			.iter()
			.any(|v| !v.is_finite() || v.abs() > 100.0)
		{
			return Err("Orbit pivot components must be finite and within -100..=100".into());
		}
		let values = [
			self.initial_yaw,
			self.initial_pitch,
			self.initial_distance,
			self.min_distance,
			self.max_distance,
			self.min_pitch,
			self.max_pitch,
			self.follow_seconds,
			self.recovery_seconds,
			self.teleport_distance,
			self.probe_radius,
		];
		if values.iter().any(|v| !v.is_finite()) {
			return Err("Orbit settings must all be finite".into());
		}
		let pi = std::f32::consts::PI;
		if !(-pi..=pi).contains(&self.initial_yaw) {
			return Err("Initial orbit yaw must be within -PI..=PI".into());
		}
		let pitch_limit = 85.0_f32.to_radians();
		if self.min_pitch < -pitch_limit
			|| self.max_pitch > pitch_limit
			|| self.min_pitch >= self.max_pitch
			|| !(self.min_pitch..=self.max_pitch).contains(&self.initial_pitch)
		{
			return Err("Orbit pitch needs ordered limits within ±85 degrees and an initial value inside them".into());
		}
		if self.min_distance < 0.25
			|| self.max_distance > 100.0
			|| self.min_distance > self.initial_distance
			|| self.initial_distance > self.max_distance
		{
			return Err("Orbit distances require 0.25 <= min <= initial <= max <= 100".into());
		}
		if !(0.0..=5.0).contains(&self.follow_seconds)
			|| !(0.0..=5.0).contains(&self.recovery_seconds)
		{
			return Err("Orbit response times must be within 0..=5 seconds".into());
		}
		if !(1.0..=1000.0).contains(&self.teleport_distance) {
			return Err("Orbit teleport distance must be within 1..=1000".into());
		}
		if !(0.01..=2.0).contains(&self.probe_radius) {
			return Err("Orbit probe radius must be within 0.01..=2".into());
		}
		if self.obstruction_mask == 0 {
			return Err("Orbit obstruction mask must include at least one layer".into());
		}
		Ok(())
	}
}

#[derive(Clone)]
pub struct GameCameraController {
	camera: Camera,
	orbit: Option<OrbitRig>,
}

#[derive(Clone)]
struct OrbitRig {
	standalone_world: Option<Arc<()>>,
	// Despawning a target prunes authored references from the runtime world.
	// Keep this run's binding and tuning independently of that document.
	settings: Arc<OrbitCameraSettings>,
	state: OrbitRuntimeState,
}

impl OrbitRig {
	fn matches_world(&self, world: &RuntimeWorld) -> bool {
		self.standalone_world
			.as_ref()
			.is_none_or(|token| Arc::ptr_eq(token, world.instance_token()))
	}
}

impl GameCameraController {
	/// Standalone controllers track the world with fixed topology. Simulation uses its live lifecycle.
	pub fn new(world: &RuntimeWorld) -> Result<Self, String> {
		if world.data().camera.orbit.is_some() {
			let mut controller = Self::new_with_lifecycle(world, &RuntimeLifecycle::new(world))?;
			if let Some(rig) = &mut controller.orbit {
				rig.standalone_world = Some(Arc::clone(world.instance_token()));
			}
			return Ok(controller);
		}
		Self::from_world(world, None)
	}

	pub(crate) fn new_with_lifecycle(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
	) -> Result<Self, String> {
		let target = world
			.data()
			.camera
			.orbit
			.as_ref()
			.and_then(|settings| lifecycle.handle(&settings.target));
		Self::from_world(world, target)
	}

	fn from_world(
		world: &RuntimeWorld,
		target: Option<RuntimeEntityHandle>,
	) -> Result<Self, String> {
		let authored = &world.data().camera;
		let template = Camera {
			position: authored.position,
			yaw: authored.yaw,
			pitch: authored.pitch,
			vertical_fov: authored.vertical_fov,
			near: authored.near_plane,
			far: authored.far_plane,
		};
		if let Some(settings) = &authored.orbit {
			let handle = target.ok_or("Orbit target is missing from the runtime lifecycle")?;
			let target = world.camera_target_snapshot(&handle)?;
			return Self::admit_orbit(template, settings, &target, 0, clear_room);
		}
		let mut controller = Self {
			camera: template,
			orbit: None,
		};
		controller.resolve(world, 0.0, true)?;
		Ok(controller)
	}

	pub fn camera(&self) -> &Camera {
		&self.camera
	}

	pub fn needs_tick(&self, world: &RuntimeWorld) -> bool {
		if let Some(rig) = &self.orbit {
			if rig.state.target_lost {
				return false;
			}
			return !rig.matches_world(world)
				|| rig.state.needs_tick()
				|| world
				.entity_world_position(&rig.state.target.key)
				.and_then(|position| desired_pivot(&rig.settings, position).ok())
				!= Some(rig.state.previous_target_pivot);
		}
		let Some(follow) = &world.data().camera.follow else {
			return false;
		};
		let Some(index) = world.entity_index(&follow.target) else {
			return false;
		};
		let desired =
			world.world_matrices()[index].w_axis.truncate() + Vec3::from_array(follow.offset);
		(Vec3::from_array(self.camera.position) - desired)
			.abs()
			.max_element()
			> camera_tolerance(desired)
	}
	/// Advance a standalone controller in a world with fixed topology.
	pub fn update(&mut self, world: &RuntimeWorld, elapsed: f32) -> Result<(), String> {
		let tick = self.orbit_state().map_or(Ok(0), |state| {
			state
				.tick
				.checked_add(1)
				.ok_or("Camera tick counter is exhausted")
		})?;
		self.advance(world, None, elapsed, CameraIntent::default(), tick)?;
		Ok(())
	}

	pub(crate) fn step(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		elapsed: f32,
		intent: CameraIntent,
		tick: u64,
	) -> Result<bool, String> {
		self.advance(world, Some(lifecycle), elapsed, intent, tick)
	}

	fn advance(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: Option<&RuntimeLifecycle>,
		elapsed: f32,
		intent: CameraIntent,
		tick: u64,
	) -> Result<bool, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Camera elapsed time must be finite and nonnegative".into());
		}
		let Some(rig) = &mut self.orbit else {
			self.resolve(world, elapsed, false)?;
			return Ok(false);
		};
		let bound = &rig.state.target;
		let position = if !rig.state.target_lost
			&& rig.matches_world(world)
			&& lifecycle.is_none_or(|live| live.validate_handle(bound).is_ok())
		{
			world.entity_world_position(&bound.key)
		} else {
			None
		};
		let result = resolve_bound_orbit(
			&rig.settings,
			&rig.state,
			&self.camera,
			BoundOrbitStep {
				target: position,
				intent,
				seconds: elapsed,
				tick,
				snap: false,
			},
			clear_room,
		)?;
		rig.state = result.state;
		self.camera = result.pose;
		Ok(result.clear_input)
	}
	fn resolve(&mut self, world: &RuntimeWorld, elapsed: f32, snap: bool) -> Result<(), String> {
		let Some(follow) = &world.data().camera.follow else {
			return Ok(());
		};
		// Runtime removal can temporarily remove the subject. Retain the last valid pose.
		let Some(index) = world.entity_index(&follow.target) else {
			return Ok(());
		};
		let target = world.world_matrices()[index].w_axis.truncate();
		let desired = target + Vec3::from_array(follow.offset);
		let alpha = if snap || follow.smoothing_seconds == 0.0 {
			1.0
		} else {
			-(-elapsed / follow.smoothing_seconds).exp_m1()
		};
		let previous = Vec3::from_array(self.camera.position);
		let mut position = previous.lerp(desired, alpha);
		if (position - desired).abs().max_element() <= camera_tolerance(desired)
			|| elapsed > 0.0 && position == previous
		{
			position = desired;
		}
		// Translate the eye and look point together while smoothing the rig.
		// Subject reversals cannot move the eye through its own look point.
		let direction = (Vec3::from_array(follow.look_offset) - Vec3::from_array(follow.offset))
			.try_normalize()
			.ok_or("Follow camera cannot resolve a viewing direction")?;
		let mut candidate = self.camera.clone();
		candidate.position = position.to_array();
		candidate.yaw = direction.x.atan2(-direction.z);
		candidate.pitch = direction.y.atan2(direction.x.hypot(direction.z));
		candidate.view_projection(1.0)?;
		self.camera = candidate;
		Ok(())
	}

	pub(crate) fn admit_orbit(
		template: Camera,
		settings: &OrbitCameraSettings,
		target: &CameraTargetSnapshot,
		tick: u64,
		query: impl FnMut([f32; 3], [f32; 3], f32) -> Result<OrbitArmLimit, String>,
	) -> Result<Self, String> {
		let initial = OrbitRuntimeState::admit(settings, target, tick)?;
		settings.validate_projection(template.near, template.vertical_fov)?;
		template.view_projection(1.0)?;
		let resolved = resolve_bound_orbit(
			settings,
			&initial,
			&template,
			BoundOrbitStep {
				target: Some(target.world_position),
				intent: CameraIntent::default(),
				seconds: 0.0,
				tick,
				snap: true,
			},
			query,
		)?;
		Ok(Self {
			camera: resolved.pose,
			orbit: Some(OrbitRig {
				standalone_world: None,
				settings: Arc::new(settings.clone()),
				state: resolved.state,
			}),
		})
	}

	pub fn orbit_state(&self) -> Option<&OrbitRuntimeState> {
		self.orbit.as_ref().map(|rig| &rig.state)
	}
}

pub fn required_probe_radius(near: f32, vertical_fov: f32) -> Result<f32, String> {
	if !near.is_finite()
		|| near <= 0.0
		|| !vertical_fov.is_finite()
		|| vertical_fov <= 0.0
		|| vertical_fov >= std::f32::consts::PI
	{
		return Err("Orbit probe requires a valid near plane and vertical FOV".into());
	}
	let near = f64::from(near);
	let half_height = near * (f64::from(vertical_fov) * 0.5).tan();
	let half_width = half_height * MAX_GAME_ASPECT;
	let required =
		(near * near + half_height * half_height + half_width * half_width).sqrt() + 0.001;
	if !required.is_finite() || required > 2.0 {
		return Err("Projection requires an orbit probe larger than supported".into());
	}
	let mut rounded = required as f32;
	if f64::from(rounded) < required {
		rounded = f32::from_bits(rounded.to_bits() + 1);
	}
	Ok(rounded)
}

impl OrbitCameraSettings {
	pub fn validate_projection(&self, near: f32, vertical_fov: f32) -> Result<(), String> {
		let required = required_probe_radius(near, vertical_fov)?;
		if self.probe_radius < required {
			return Err(format!(
				"Orbit probe radius must be at least {required} for this projection"
			));
		}
		Ok(())
	}
}

fn camera_tolerance(position: Vec3) -> f32 {
	(4.0 * f32::EPSILON * position.abs().max_element()).max(0.0001)
}

fn clear_room(
	_pivot: [f32; 3],
	_forward: [f32; 3],
	distance: f32,
) -> Result<OrbitArmLimit, String> {
	Ok(OrbitArmLimit {
		distance,
		hit: None,
	})
}

#[derive(Clone, Debug)]
pub struct CameraTargetSnapshot {
	pub handle: RuntimeEntityHandle,
	pub world_position: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrbitRuntimeState {
	pub target: RuntimeEntityHandle,
	pub target_lost: bool,
	pub yaw: f32,
	pub pitch: f32,
	pub user_distance: f32,
	pub effective_distance: f32,
	pub filtered_pivot: [f32; 3],
	pub previous_target_pivot: [f32; 3],
	pub last_allowed_distance: f32,
	pub tick: u64,
	pub obstruction: Option<CameraProbeHit>,
	pub pivot_settled: bool,
	pub arm_settled: bool,
}

impl OrbitRuntimeState {
	pub fn needs_tick(&self) -> bool {
		!self.target_lost && (!self.pivot_settled || !self.arm_settled)
	}

	/// Host diagnostics exclude continuous pose movement and the fixed-step clock.
	pub fn same_status(&self, other: &Self) -> bool {
		self.target_lost == other.target_lost
			&& self.user_distance == other.user_distance
			&& self.last_allowed_distance == other.last_allowed_distance
			&& self.obstruction == other.obstruction
			&& self.pivot_settled == other.pivot_settled
			&& self.arm_settled == other.arm_settled
	}

	pub fn admit(
		settings: &OrbitCameraSettings,
		target: &CameraTargetSnapshot,
		tick: u64,
	) -> Result<Self, String> {
		settings.validate()?;
		if target.handle.key != settings.target {
			return Err("Snapshot does not match authored orbit target".into());
		}
		let pivot = desired_pivot(settings, target.world_position)?;
		Ok(Self {
			target: target.handle.clone(),
			target_lost: false,
			yaw: normalize_yaw(f64::from(settings.initial_yaw))?,
			pitch: settings.initial_pitch,
			user_distance: settings.initial_distance,
			effective_distance: settings.initial_distance,
			filtered_pivot: pivot,
			previous_target_pivot: pivot,
			last_allowed_distance: settings.initial_distance,
			tick,
			obstruction: None,
			pivot_settled: true,
			arm_settled: true,
		})
	}
}

pub struct OrbitStepInput<'a> {
	pub target: Option<&'a CameraTargetSnapshot>,
	pub intent: CameraIntent,
	pub step_seconds: f32,
	pub tick: u64,
	pub snap: bool,
}

pub struct OrbitArmLimit {
	pub distance: f32,
	pub hit: Option<CameraProbeHit>,
}

pub struct OrbitStepResult {
	pub state: OrbitRuntimeState,
	pub pose: Camera,
	pub clear_input: bool,
}

fn checked_vec3(value: DVec3, label: &str) -> Result<[f32; 3], String> {
	if !value.is_finite() || value.abs().max_element() > f64::from(f32::MAX) {
		return Err(format!("{label} exceeds finite camera coordinates"));
	}
	let result = value.as_vec3();
	if !result.is_finite() {
		return Err(format!("{label} cannot fit f32"));
	}
	Ok(result.to_array())
}

fn desired_pivot(settings: &OrbitCameraSettings, position: [f32; 3]) -> Result<[f32; 3], String> {
	checked_vec3(
		DVec3::from_array(position.map(f64::from))
			+ DVec3::from_array(settings.pivot_offset.map(f64::from)),
		"Camera pivot",
	)
}

fn settle_tolerance(value: f32) -> f32 {
	(4.0 * f32::EPSILON * value.abs()).max(0.0001)
}

fn smoothing_alpha(dt: f64, response: f64) -> Result<f64, String> {
	if !dt.is_finite() || dt < 0.0 || !response.is_finite() || response < 0.0 {
		return Err("Smoothing time must be finite and nonnegative".into());
	}
	Ok(if response == 0.0 {
		1.0
	} else {
		-(-dt / response).exp_m1()
	})
}

fn smooth_scalar(previous: f32, desired: f32, alpha: f64) -> f32 {
	if alpha == 0.0 {
		return previous;
	}
	if alpha == 1.0 {
		return desired;
	}
	let candidate =
		(f64::from(previous) + (f64::from(desired) - f64::from(previous)) * alpha) as f32;
	if (candidate - desired).abs() <= settle_tolerance(desired) || candidate == previous {
		desired
	} else {
		candidate
	}
}

fn normalize_yaw(value: f64) -> Result<f32, String> {
	if !value.is_finite() {
		return Err("Camera yaw is not finite".into());
	}
	if value.abs() == f64::from(std::f32::consts::PI) {
		return Ok(-std::f32::consts::PI);
	}
	let pi = std::f64::consts::PI;
	let result = ((value + pi).rem_euclid(std::f64::consts::TAU) - pi) as f32;
	// Narrowing can round a value just below +pi up to +f32::PI.
	Ok(if result >= std::f32::consts::PI {
		-std::f32::consts::PI
	} else {
		result
	})
}

pub fn orbit_pose(
	template: &Camera,
	pivot: [f32; 3],
	yaw: f32,
	pitch: f32,
	distance: f32,
) -> Result<Camera, String> {
	if !pivot.iter().all(|v| v.is_finite())
		|| !yaw.is_finite()
		|| !pitch.is_finite()
		|| !distance.is_finite()
		|| distance < 0.0
	{
		return Err("Orbit inputs must be finite and distance nonnegative".into());
	}
	let mut pose = template.clone();
	pose.yaw = yaw;
	pose.pitch = pitch;
	pose.position = orbit_eye(pivot, pose.forward(), distance)?;
	pose.view_projection(1.0)?;
	Ok(pose)
}

fn orbit_eye(pivot: [f32; 3], forward: [f32; 3], distance: f32) -> Result<[f32; 3], String> {
	let origin = DVec3::from_array(pivot.map(f64::from));
	let direction = DVec3::from_array(forward.map(f64::from));
	checked_vec3(origin - direction * f64::from(distance), "Camera eye")
}

/// Resolve one fixed step against a caller-supplied arm query without mutating the accepted rig.
pub fn resolve_orbit(
	settings: &OrbitCameraSettings,
	previous: &OrbitRuntimeState,
	template: &Camera,
	step: OrbitStepInput<'_>,
	query: impl FnMut([f32; 3], [f32; 3], f32) -> Result<OrbitArmLimit, String>,
) -> Result<OrbitStepResult, String> {
	settings.validate()?;
	settings.validate_projection(template.near, template.vertical_fov)?;
	template.view_projection(1.0)?;
	let target = step
		.target
		.filter(|target| target.handle == previous.target)
		.map(|target| target.world_position);
	resolve_bound_orbit(
		settings,
		previous,
		template,
		BoundOrbitStep {
			target,
			intent: step.intent,
			seconds: step.step_seconds,
			tick: step.tick,
			snap: step.snap,
		},
		query,
	)
}

struct BoundOrbitStep {
	target: Option<[f32; 3]>,
	intent: CameraIntent,
	seconds: f32,
	tick: u64,
	snap: bool,
}

fn resolve_bound_orbit(
	settings: &OrbitCameraSettings,
	previous: &OrbitRuntimeState,
	template: &Camera,
	step: BoundOrbitStep,
	mut query: impl FnMut([f32; 3], [f32; 3], f32) -> Result<OrbitArmLimit, String>,
) -> Result<OrbitStepResult, String> {
	let BoundOrbitStep {
		target: position,
		intent,
		seconds: step_seconds,
		tick,
		snap,
	} = step;
	if !step_seconds.is_finite() || step_seconds < 0.0 {
		return Err("Camera step duration must be finite and non-negative".into());
	}
	if tick < previous.tick {
		return Err("Camera tick cannot move backwards".into());
	}
	let mut next = previous.clone();
	next.tick = tick;
	let Some(position) = position.filter(|_| !previous.target_lost) else {
		next.target_lost = true;
		next.pivot_settled = true;
		next.arm_settled = true;
		return Ok(OrbitStepResult {
			state: next,
			pose: template.clone(),
			clear_input: true,
		});
	};
	let pivot = desired_pivot(settings, position)?;
	let displacement = DVec3::from_array(pivot.map(f64::from)).distance(DVec3::from_array(
		previous.previous_target_pivot.map(f64::from),
	));
	let snap = snap || displacement >= f64::from(settings.teleport_distance);
	let alpha = if snap {
		1.0
	} else {
		smoothing_alpha(f64::from(step_seconds), f64::from(settings.follow_seconds))?
	};
	for (axis, target) in next.filtered_pivot.iter_mut().zip(pivot) {
		*axis = smooth_scalar(*axis, target, alpha);
	}
	next.previous_target_pivot = pivot;
	next.pivot_settled = next.filtered_pivot == pivot;
	intent.validate_step()?;
	if intent.yaw_delta != 0.0 {
		next.yaw = normalize_yaw(f64::from(previous.yaw) + intent.yaw_delta)?;
	}
	next.pitch = (f64::from(previous.pitch) + intent.pitch_delta)
		.clamp(f64::from(settings.min_pitch), f64::from(settings.max_pitch)) as f32;
	if !previous.user_distance.is_finite() || previous.user_distance <= 0.0 {
		return Err("Orbit runtime has an invalid desired distance".into());
	}
	let zoom = f64::from(previous.user_distance) * intent.log_distance_delta.exp();
	if !zoom.is_finite() {
		return Err("Orbit zoom overflowed".into());
	}
	next.user_distance = zoom.clamp(
		f64::from(settings.min_distance),
		f64::from(settings.max_distance),
	) as f32;

	let mut pose = template.clone();
	pose.yaw = next.yaw;
	pose.pitch = next.pitch;
	let forward = pose.forward();
	let limit = query(next.filtered_pivot, forward, next.user_distance)?;
	if !limit.distance.is_finite() || limit.distance < 0.0 || limit.distance > next.user_distance {
		return Err("Camera query returned an invalid allowed distance".into());
	}
	if let Some(hit) = &limit.hit {
		hit.blocker.validate()?;
		if !hit.fraction.is_finite() || !(0.0..=1.0).contains(&hit.fraction) {
			return Err("Camera query returned an invalid hit fraction".into());
		}
	}
	next.effective_distance = if snap || limit.distance <= previous.effective_distance {
		limit.distance
	} else {
		let recovery = smoothing_alpha(
			f64::from(step_seconds),
			f64::from(settings.recovery_seconds),
		)?;
		smooth_scalar(previous.effective_distance, limit.distance, recovery).min(limit.distance)
	};
	next.last_allowed_distance = limit.distance;
	next.arm_settled = next.effective_distance == limit.distance;
	next.obstruction = limit.hit;
	pose.position = orbit_eye(next.filtered_pivot, forward, next.effective_distance)?;
	// Admission validates the initial template. A stationary step can reuse the accepted
	// projection even when the query changes a diagnostic such as the blocker identity.
	if pose.position != template.position
		|| pose.yaw != template.yaw
		|| pose.pitch != template.pitch
	{
		pose.view_projection(1.0)?;
	}
	Ok(OrbitStepResult {
		state: next,
		pose,
		clear_input: false,
	})
}

#[cfg(test)]
mod orbit_math_tests {
	use super::*;
	use crate::{
		SceneData,
		scene_data::{CURRENT_SCHEMA_VERSION, CameraData, EntityData},
	};
	use std::collections::HashSet;
	use std::f32::consts::{FRAC_PI_2, FRAC_PI_6, PI};

	fn settings() -> OrbitCameraSettings {
		OrbitCameraSettings {
			target: EntityKey::new("actor").unwrap(),
			pivot_offset: [0.0; 3],
			initial_yaw: 0.0,
			initial_pitch: 0.0,
			initial_distance: 5.0,
			min_pitch: -1.0,
			max_pitch: 1.0,
			min_distance: 1.0,
			max_distance: 10.0,
			follow_seconds: 0.1,
			recovery_seconds: 0.2,
			teleport_distance: 10.0,
			probe_radius: 0.25,
			obstruction_mask: 1,
		}
	}

	fn subject(settings: &OrbitCameraSettings) -> CameraTargetSnapshot {
		CameraTargetSnapshot {
			handle: RuntimeEntityHandle {
				world_id: "test".into(),
				key: settings.target.clone(),
				generation: 1,
			},
			world_position: [0.0; 3],
		}
	}

	fn fixed_step(target: &CameraTargetSnapshot, tick: u64) -> OrbitStepInput<'_> {
		OrbitStepInput {
			target: Some(target),
			intent: CameraIntent::default(),
			tick,
			step_seconds: 1.0 / 60.0,
			snap: false,
		}
	}

	fn close(actual: f32, expected: f32) {
		assert!((actual - expected).abs() < 1.0e-5, "{actual} != {expected}");
	}

	#[test]
	fn cardinal_angles_and_zero_arm_retain_forward_orientation() {
		let template = Camera::default();
		for (yaw, pitch, expected) in [
			(0.0, 0.0, [0.0, 0.0, 5.0]),
			(FRAC_PI_2, 0.0, [-5.0, 0.0, 0.0]),
			(0.0, -FRAC_PI_6, [0.0, 2.5, 4.330127]),
		] {
			let pose = orbit_pose(&template, [0.0; 3], yaw, pitch, 5.0).unwrap();
			for (actual, expected) in pose.position.into_iter().zip(expected) {
				close(actual, expected);
			}
		}
		let pose = orbit_pose(&template, [1.0, 2.0, 3.0], 0.4, -0.2, 0.0).unwrap();
		assert_eq!(
			(pose.position, pose.yaw, pose.pitch),
			([1.0, 2.0, 3.0], 0.4, -0.2)
		);
		assert!(orbit_pose(&template, [f32::MAX; 3], 0.0, 0.0, 5.0).is_err());
		assert!(orbit_pose(&template, [0.0; 3], 0.0, 0.0, f32::NAN).is_err());
	}

	#[test]
	fn exponential_response_settles_exactly_and_zero_elapsed_preserves_residuals() {
		assert_eq!(smoothing_alpha(0.0, 0.1).unwrap(), 0.0);
		assert_eq!(smoothing_alpha(0.1, 0.0).unwrap(), 1.0);
		assert!(smoothing_alpha(f64::NAN, 0.1).is_err());
		let alpha = smoothing_alpha(1.0 / 60.0, 0.1).unwrap();
		let mut value = smooth_scalar(0.0, 1.0, alpha);
		close(value, 0.15351827);
		assert_eq!(smooth_scalar(value, value + 0.00001, 0.0), value);
		for _ in 1..240 {
			value = smooth_scalar(value, 1.0, alpha);
		}
		assert_eq!(value, 1.0);
		assert_eq!(smooth_scalar(f32::MAX, 1.0, 1.0), 1.0);
	}

	#[test]
	fn both_signed_pi_defaults_stay_canonical_on_empty_steps() {
		let template = Camera::default();
		for endpoint in [-PI, PI] {
			let mut settings = settings();
			settings.initial_yaw = endpoint;
			let target = subject(&settings);
			let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
			assert_eq!(initial.yaw, -PI);
			let step = resolve_orbit(
				&settings,
				&initial,
				&template,
				fixed_step(&target, 1),
				clear_room,
			)
				.unwrap();
			assert_eq!(step.pose.yaw, -PI);
			assert!(initial.same_status(&step.state));
		}
		assert!(normalize_yaw(f64::INFINITY).is_err());
		close(normalize_yaw(3.0 * std::f64::consts::PI).unwrap(), -PI);
	}

	#[test]
	fn queried_arm_shortens_immediately_and_recovers_on_safe_segment() {
		let settings = settings();
		let target = subject(&settings);
		let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
		let hit = CameraProbeHit {
			blocker: EntityKey::new("wall").unwrap(),
			fraction: 0.4,
		};
		let query = |_, _, distance| {
			assert_eq!(distance, 5.0);
			Ok(OrbitArmLimit {
				distance: 2.0,
				hit: Some(hit.clone()),
			})
		};
		let blocked = resolve_orbit(
			&settings,
			&initial,
			&Camera::default(),
			fixed_step(&target, 1),
			query,
		)
			.unwrap();
		assert_eq!(blocked.state.effective_distance, 2.0);
		assert_eq!(blocked.state.user_distance, 5.0);
		assert_eq!(blocked.state.obstruction.as_ref(), Some(&hit));
		assert!(!blocked.state.needs_tick());
		let stationary = resolve_orbit(
			&settings,
			&blocked.state,
			&blocked.pose,
			fixed_step(&target, 2),
			query,
		)
			.unwrap();
		assert!(blocked.state.same_status(&stationary.state));
		let recovered = resolve_orbit(
			&settings,
			&blocked.state,
			&blocked.pose,
			fixed_step(&target, 2),
			clear_room,
		)
			.unwrap();
		assert!(!blocked.state.same_status(&recovered.state));
		assert!(
			recovered.state.effective_distance > 2.0 && recovered.state.effective_distance < 5.0
		);
		assert_eq!(recovered.state.last_allowed_distance, 5.0);
		assert!(recovered.state.needs_tick());
		for invalid in [f32::NAN, -0.1, 6.0] {
			assert!(
				resolve_orbit(
					&settings,
					&initial,
					&Camera::default(),
					fixed_step(&target, 1),
					|_, _, _| Ok(OrbitArmLimit {
						distance: invalid,
						hit: None
					})
				)
					.is_err()
			);
		}
		assert!(
			resolve_orbit(
				&settings,
				&initial,
				&Camera::default(),
				fixed_step(&target, 1),
				|_, _, _| Ok(OrbitArmLimit {
					distance: 2.0,
					hit: Some(CameraProbeHit {
						fraction: 1.2,
						..hit.clone()
					})
				})
			)
				.is_err()
		);
	}

	#[test]
	fn teleport_threshold_snaps_before_the_query_and_identity_mismatch_freezes_pose() {
		let settings = settings();
		let mut target = subject(&settings);
		let initial = OrbitRuntimeState::admit(&settings, &target, 0).unwrap();
		target.world_position[0] = settings.teleport_distance;
		let teleported = resolve_orbit(
			&settings,
			&initial,
			&Camera::default(),
			fixed_step(&target, 1),
			|pivot, forward, distance| {
				assert_eq!(pivot, target.world_position);
				assert_eq!(forward, [0.0, 0.0, -1.0]);
				Ok(OrbitArmLimit {
					distance,
					hit: None,
				})
			},
		)
			.unwrap();
		assert_eq!(teleported.state.filtered_pivot, target.world_position);
		target.handle.generation += 1;
		let lost = resolve_orbit(
			&settings,
			&teleported.state,
			&teleported.pose,
			fixed_step(&target, 2),
			|_, _, _| panic!("lost binding must skip query"),
		)
			.unwrap();
		assert!(lost.clear_input && lost.state.target_lost && !lost.state.needs_tick());
		assert_eq!(lost.pose.position, teleported.pose.position);
	}

	fn orbit_document() -> Arc<SceneData> {
		let settings = settings();
		let target = settings.target.clone();
		let template = Camera::default();
		Arc::new(SceneData {
			schema_version: CURRENT_SCHEMA_VERSION,
			bindings: Default::default(),
			lights: vec![],
			entities: vec![EntityData::new(
				target.clone(),
				"Actor",
				"models/actor.gltf",
			)],
			camera: CameraData {
				follow: None,
				orbit: Some(settings),
				position: template.position,
				yaw: template.yaw,
				pitch: template.pitch,
				vertical_fov: template.vertical_fov,
				near_plane: template.near,
				far_plane: template.far,
			},
		})
	}

	#[test]
	fn independent_world_with_the_same_key_cannot_take_over_a_standalone_rig() {
		let document = orbit_document();
		let mut original = RuntimeWorld::new(document.clone()).unwrap();
		let mut replacement = RuntimeWorld::new(document).unwrap();
		let target = settings().target;
		let mut controller = GameCameraController::new(&original).unwrap();
		let accepted = controller.camera().position;
		// Even identical coordinates need one step to publish terminal target loss.
		assert!(!controller.needs_tick(&original));
		assert!(controller.needs_tick(&replacement));
		let transform = crate::scene_data::TransformData {
			position: [8.0, 0.0, 0.0],
			..Default::default()
		};
		replacement
			.set_transform(&target, transform.clone())
			.unwrap();
		controller.update(&replacement, 1.0).unwrap();
		assert!(controller.orbit_state().unwrap().target_lost);
		assert_eq!(controller.camera().position, accepted);
		assert!(!controller.needs_tick(&replacement));

		let mut fresh = GameCameraController::new(&original).unwrap();
		original.set_transform(&target, transform).unwrap();
		assert!(fresh.needs_tick(&original));
		fresh.update(&original, 1.0).unwrap();
		assert!(!fresh.orbit_state().unwrap().target_lost);
		assert!(fresh.camera().position[0] > accepted[0]);
	}

	#[test]
	fn lifecycle_removal_clears_input_freezes_pose_and_never_rebinds() {
		let document = orbit_document();
		let target = settings().target;
		let mut world = RuntimeWorld::new(document.clone()).unwrap();
		let mut lifecycle = RuntimeLifecycle::new(&world);
		let mut controller = GameCameraController::new_with_lifecycle(&world, &lifecycle).unwrap();
		let yaw = CameraIntent {
			yaw_delta: 0.25,
			..Default::default()
		};
		assert!(
			!controller
				.step(&world, &lifecycle, 1.0 / 60.0, yaw, 1)
				.unwrap()
		);
		let accepted = controller.camera().clone();
		let removed = HashSet::from([target]);
		lifecycle.removed(&removed).unwrap();
		world.remove_runtime_entities(&removed).unwrap();
		assert!(world.data().camera.orbit.is_none());
		assert!(controller.needs_tick(&world));
		assert!(
			controller
				.step(&world, &lifecycle, 1.0 / 60.0, yaw, 2)
				.unwrap()
		);
		assert!(controller.orbit_state().unwrap().target_lost);
		assert!(!controller.needs_tick(&world));
		let replacement = RuntimeWorld::new(document).unwrap();
		let new_lifecycle = RuntimeLifecycle::new(&replacement);
		assert!(
			controller
				.step(&replacement, &new_lifecycle, 1.0 / 60.0, yaw, 3)
				.unwrap()
		);
		assert_eq!(controller.camera().position, accepted.position);
		assert_eq!(controller.camera().yaw, accepted.yaw);
		assert!(!controller.needs_tick(&replacement));
	}
}
