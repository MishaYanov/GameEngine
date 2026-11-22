use super::{GameCameraController, follow, obstruction_query};
use crate::collision::{CameraExclusions, CameraProbeHit, CollisionWorld};
use crate::runtime_lifecycle::{RuntimeEntityHandle, RuntimeLifecycle};
use crate::{
	Camera, EntityKey, SceneData, camera_input::CameraIntent,
	simulation_session::SimulationSession, world::RuntimeWorld,
};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};

pub const MAX_GAME_ASPECT: f64 = 3.0;

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

#[derive(Clone)]
pub(super) struct OrbitRig {
	standalone: Option<Arc<StandaloneProbe>>,
	// Despawning a target prunes authored references from the runtime world.
	// Keep this run's binding and tuning independently of that document.
	settings: Arc<OrbitCameraSettings>,
	state: OrbitRuntimeState,
}

impl OrbitRig {
	pub(super) fn needs_tick(&self, world: &RuntimeWorld) -> bool {
		if self.state.target_lost {
			return false;
		}
		self.standalone
			.as_ref()
			.is_some_and(|source| !source.matches_world(world) || !source.is_current(world))
			|| self.state.needs_tick()
			|| world
				.entity_world_position(&self.state.target.key)
				.and_then(|position| desired_pivot(&self.settings, position).ok())
				!= Some(self.state.previous_target_pivot)
	}
}

#[derive(Clone)]
struct StandaloneProbe {
	world_token: Arc<()>,
	document: Weak<SceneData>,
	lifecycle: Arc<RuntimeLifecycle>,
	collisions: Arc<CollisionWorld>,
	exclusions: Option<CameraExclusions>,
}

impl StandaloneProbe {
	fn matches_world(&self, world: &RuntimeWorld) -> bool {
		Arc::ptr_eq(&self.world_token, world.instance_token())
	}

	fn is_current(&self, world: &RuntimeWorld) -> bool {
		// World mutations use Arc::make_mut, which detaches weak document references.
		// Holding a Weak also avoids forcing a document clone on every transform edit.
		std::ptr::eq(self.document.as_ptr(), world.data())
	}
}

impl GameCameraController {
	/// Standalone controllers maintain a probe for the supplied world. Simulation shares its own collision cache.
	pub fn new(world: &RuntimeWorld) -> Result<Self, String> {
		if world.data().camera.orbit.is_none() {
			return Self::fixed_or_follow(world, &world.data().camera);
		}
		let mut standalone = StandaloneProbe {
			world_token: Arc::clone(world.instance_token()),
			document: Arc::downgrade(&world.document()),
			lifecycle: Arc::new(RuntimeLifecycle::new(world)),
			collisions: Arc::new(CollisionWorld::new(world)?),
			exclusions: None,
		};
		let mut remaining = SimulationSession::MAX_COLLISION_CANDIDATES_PER_TICK;
		let mut controller = Self::new_with_lifecycle(
			world,
			&standalone.lifecycle,
			obstruction_query(
				world,
				&standalone.lifecycle,
				&standalone.collisions,
				&mut standalone.exclusions,
				&mut remaining,
			),
		)?;
		if let Some(rig) = &mut controller.orbit {
			rig.standalone = Some(Arc::new(standalone));
		}
		Ok(controller)
	}

	pub(crate) fn new_with_lifecycle(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		query: impl FnMut(
			&RuntimeEntityHandle,
			&OrbitCameraSettings,
			[f32; 3],
			[f32; 3],
			f32,
		) -> Result<OrbitArmLimit, String>,
	) -> Result<Self, String> {
		Self::from_authored(world, lifecycle, &world.data().camera, 0, query)
	}

	pub(crate) fn from_authored(
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		authored: &crate::scene_data::CameraData,
		tick: u64,
		mut query: impl FnMut(
			&RuntimeEntityHandle,
			&OrbitCameraSettings,
			[f32; 3],
			[f32; 3],
			f32,
		) -> Result<OrbitArmLimit, String>,
	) -> Result<Self, String> {
		let Some(settings) = &authored.orbit else {
			return Self::fixed_or_follow(world, authored);
		};
		let handle = lifecycle
			.handle(&settings.target)
			.ok_or("Orbit target is missing from the runtime lifecycle")?;
		let target = world.camera_target_snapshot(&handle)?;
		Self::admit_orbit(
			authored.to_camera(),
			settings,
			&target,
			tick,
			|pivot, forward, distance| query(&target.handle, settings, pivot, forward, distance),
		)
	}

	/// Advance against current world geometry and retain the last accepted cache and pose if the probe fails.
	pub fn update(&mut self, world: &RuntimeWorld, elapsed: f32) -> Result<(), String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Camera elapsed time must be finite and nonnegative".into());
		}
		let Some(rig) = self.orbit.as_ref() else {
			return follow::step(
				&mut self.camera,
				self.follow.as_ref(),
				world,
				elapsed,
				false,
			);
		};
		let tick = rig
			.state
			.tick
			.checked_add(1)
			.ok_or("Camera tick counter is exhausted")?;
		let source = rig
			.standalone
			.as_ref()
			.ok_or("Session orbit updates require the active runtime lifecycle")?;
		let refresh =
			!rig.state.target_lost && source.matches_world(world) && !source.is_current(world);
		let mut candidate = (**source).clone();
		if refresh {
			candidate.collisions = Arc::new(CollisionWorld::new(world)?);
			candidate.document = Arc::downgrade(&world.document());
		}
		let mut remaining = SimulationSession::MAX_COLLISION_CANDIDATES_PER_TICK;
		self.step(
			world,
			&candidate.lifecycle,
			elapsed,
			CameraIntent::default(),
			tick,
			obstruction_query(
				world,
				&candidate.lifecycle,
				&candidate.collisions,
				&mut candidate.exclusions,
				&mut remaining,
			),
		)?;
		if refresh && let Some(rig) = &mut self.orbit {
			rig.standalone = Some(Arc::new(candidate));
		}
		Ok(())
	}

	pub(crate) fn step(
		&mut self,
		world: &RuntimeWorld,
		lifecycle: &RuntimeLifecycle,
		elapsed: f32,
		intent: CameraIntent,
		tick: u64,
		mut query: impl FnMut(
			&RuntimeEntityHandle,
			&OrbitCameraSettings,
			[f32; 3],
			[f32; 3],
			f32,
		) -> Result<OrbitArmLimit, String>,
	) -> Result<bool, String> {
		if !elapsed.is_finite() || elapsed < 0.0 {
			return Err("Camera elapsed time must be finite and nonnegative".into());
		}
		let Some(rig) = &mut self.orbit else {
			follow::step(
				&mut self.camera,
				self.follow.as_ref(),
				world,
				elapsed,
				false,
			)?;
			return Ok(false);
		};
		let bound = &rig.state.target;
		let position = if !rig.state.target_lost
			&& rig
				.standalone
				.as_ref()
				.is_none_or(|source| source.matches_world(world))
			&& lifecycle.validate_handle(bound).is_ok()
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
			|pivot, forward, distance| query(bound, &rig.settings, pivot, forward, distance),
		)?;
		rig.state = result.state;
		self.camera = result.pose;
		Ok(result.clear_input)
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
			follow: None,
			orbit: Some(OrbitRig {
				standalone: None,
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
#[path = "../../tests/unit/game_camera/orbit/orbit_math_tests.rs"]
mod orbit_math_tests;
