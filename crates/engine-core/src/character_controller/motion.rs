use super::{CharacterControllerSettings, CharacterState};
use crate::{
	EntityKey, animation::RootMotionDelta, collision::CollisionWorld, world::RuntimeWorld,
};
use glam::Vec3;

fn deadline(tick: u64, window: u32) -> Result<u64, String> {
	tick.checked_add(u64::from(window))
		.ok_or_else(|| "Character deadline overflow".into())
}
fn live(until: Option<u64>, tick: u64) -> bool {
	until.is_some_and(|end| tick <= end)
}
fn approach(current: Vec3, target: Vec3, maximum: f32) -> Vec3 {
	let difference = target - current;
	let length = difference.length();
	if length <= maximum || length == 0. {
		target
	} else {
		current + difference * (maximum / length)
	}
}
fn supported(cast: &crate::collision::BodyCast, body: crate::Bounds3, offset: Vec3) -> bool {
	cast.flat_support
		&& cast.obstacle_bounds.is_some_and(|top| {
			[0, 2].into_iter().all(|axis| {
				(body.max()[axis] + offset[axis]).min(top.max()[axis])
					- (body.min()[axis] + offset[axis]).max(top.min()[axis])
					>= 0.001
			})
		})
}
#[allow(clippy::too_many_arguments)]
pub(super) fn solve(
	world: &RuntimeWorld,
	collisions: &CollisionWorld,
	key: &EntityKey,
	old: &CharacterState,
	movement: [f32; 2],
	press: bool,
	s: &CharacterControllerSettings,
	tick: u64,
	dt: f32,
	root_motion: Option<RootMotionDelta>,
	budget: &mut usize,
) -> Result<(Vec3, CharacterState), String> {
	let mut next = old.clone();
	let body = collisions.body_bounds(key)?;
	let pre = collisions.cast_body(world, key, Vec3::ZERO, -Vec3::Y * s.ground_probe, budget)?;
	let grounded = old.velocity[1] <= 0. && supported(&pre, body, Vec3::ZERO);
	if grounded {
		next.jump_available = true;
		next.coyote_until = Some(deadline(tick, s.coyote_ticks)?);
	}
	if press {
		next.jump_buffer_until = Some(deadline(tick, s.jump_buffer_ticks)?);
	}
	let jump = next.jump_available
		&& (grounded || live(next.coyote_until, tick))
		&& live(next.jump_buffer_until, tick);
	let mut velocity = Vec3::from_array(old.velocity);
	let horizontal = approach(
		Vec3::new(velocity.x, 0., velocity.z),
		Vec3::new(movement[0], 0., movement[1]) * s.walk_speed,
		if grounded && !jump {
			s.ground_acceleration * dt
		} else {
			s.air_acceleration * dt
		},
	);
	velocity.x = horizontal.x;
	velocity.z = horizontal.z;
	if jump {
		velocity.y = s.jump_speed;
		next.jump_available = false;
		next.coyote_until = None;
		next.jump_buffer_until = None;
		next.jumped_at_tick = Some(tick);
	} else if grounded {
		velocity.y = 0.;
	}
	velocity.y = (velocity.y - s.gravity * dt).max(-s.max_fall_speed);
	let impact = (-velocity.y).max(0.);
	let mut requested = velocity * dt;
	if let Some(motion) = root_motion {
		// Root displacement contributes only to this cast. The motor retains its own acceleration history.
		requested.x =
			requested.x * (1.0 - motion.motor_weight) + motion.displacement.x * motion.motor_weight;
		requested.z =
			requested.z * (1.0 - motion.motor_weight) + motion.displacement.y * motion.motor_weight;
	}
	let (mut applied, contacts) = collisions.slide_body(world, key, requested, budget)?;
	let wall = contacts.iter().any(|c| c.normal[1].abs() < 0.01);
	for contact in &contacts {
		let normal = Vec3::from_array(contact.normal);
		velocity -= normal * velocity.dot(normal).max(0.);
	}
	let mut step_support = None;
	let planar = Vec3::new(requested.x, 0., requested.z);
	if grounded && !jump && wall && s.step_height > 0. && planar.length_squared() > 1e-12 {
		let up = collisions.cast_body(world, key, Vec3::ZERO, Vec3::Y * s.step_height, budget)?;
		if up.contact.is_none() {
			let forward = collisions.cast_body(world, key, up.applied, planar, budget)?;
			if forward.contact.is_none() {
				let raised = up.applied + forward.applied;
				let down = collisions.cast_body(
					world,
					key,
					raised,
					-Vec3::Y * (s.step_height + s.ground_probe),
					budget,
				)?;
				let candidate = raised + down.applied;
				if supported(&down, body, candidate)
					&& candidate.y <= s.step_height + 0.0002
					&& candidate.y >= -s.ground_probe - 0.0002
					&& candidate.dot(planar.normalize()) > applied.dot(planar.normalize()) + 0.0002
				{
					applied = candidate;
					step_support = down.contact.map(|c| c.entity);
					velocity = horizontal;
				}
			}
		}
	}
	next.grounded = false;
	next.support = None;
	if let Some(support) = step_support {
		next.grounded = true;
		next.support = Some(support);
	} else if !jump && velocity.y <= 0. {
		let post = collisions.cast_body(world, key, applied, -Vec3::Y * s.ground_probe, budget)?;
		if supported(&post, body, applied)
			&& (grounded || contacts.iter().any(|c| c.normal[1] < -0.999) || !old.initialized)
		{
			applied += post.applied;
			next.grounded = true;
			next.support = post.contact.map(|c| c.entity);
			velocity.y = 0.;
		}
	}
	if next.grounded {
		next.jump_available = true;
		if old.initialized && !old.grounded {
			next.landed_at_tick = Some(tick);
			next.impact_speed = impact;
		}
	}
	next.velocity = velocity.to_array();
	next.planar_speed = Vec3::new(applied.x, 0., applied.z).length() / dt;
	next.initialized = true;
	next.tick = tick;
	if !applied.is_finite() || !velocity.is_finite() {
		return Err("Character motion is nonfinite".into());
	}
	Ok((applied, next))
}
