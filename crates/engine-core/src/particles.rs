//! Committed world-space particle presentation, independent of renderer resources.

use crate::{
	EntityKey, components::ParticleEmitterSettings, scene_data::EntityData, world::RuntimeWorld,
};
use glam::{EulerRot, Quat, Vec3};
use std::collections::{BTreeMap, HashMap};

pub const MAX_PARTICLES: usize = 2048;
const MAX_PARTICLES_PER_EMITTER: usize = 192;

/// A renderer-independent sample. Size is the billboard diameter in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
	pub position: [f32; 3],
	pub size: f32,
	pub color: [f32; 3],
}

#[derive(Clone)]
struct Particle {
	origin: Vec3,
	velocity: Vec3,
	age_steps: u32,
}

impl Particle {
	fn sample(&self, settings: &ParticleEmitterSettings, fixed_hz: u32) -> ParticleSample {
		let age = self.age_steps as f32 / fixed_hz as f32;
		let normalized_age = (age / settings.lifetime_seconds).min(1.0);
		let acceleration = Vec3::Y * (0.5 * settings.gravity * age * age);
		ParticleSample {
			position: (self.origin + self.velocity * age + acceleration).to_array(),
			size: settings.size * (1.0 - 0.75 * normalized_age),
			color: settings.color,
		}
	}
}

#[derive(Clone)]
struct Emitter {
	settings: ParticleEmitterSettings,
	random_state: u32,
	birth_credit: f64,
	particles: Vec<Particle>,
}

impl Emitter {
	fn new(settings: ParticleEmitterSettings) -> Self {
		Self {
			random_state: settings.seed,
			birth_credit: 0.0,
			settings,
			particles: Vec::new(),
		}
	}

	fn age(&mut self, fixed_hz: u32) {
		self.particles.retain_mut(|particle| {
			particle.age_steps += 1;
			(particle.age_steps as f32 / fixed_hz as f32) < self.settings.lifetime_seconds
		});
	}

	fn accepted_births(&mut self, fixed_hz: u32, free: usize) -> usize {
		let fixed_hz = f64::from(fixed_hz);
		self.birth_credit += f64::from(self.settings.rate_per_second);
		let births = (self.birth_credit / fixed_hz).floor() as usize;
		self.birth_credit -= births as f64 * fixed_hz;
		births
			.min(free)
			.min(MAX_PARTICLES_PER_EMITTER - self.particles.len())
	}

	fn emit(&mut self, births: usize, origin: Vec3, rotation: Quat) {
		for _ in 0..births {
			let velocity = self.velocity(rotation);
			self.particles.push(Particle {
				origin,
				velocity,
				age_steps: 0,
			});
		}
	}

	fn velocity(&mut self, rotation: Quat) -> Vec3 {
		let azimuth = self.random() * std::f32::consts::TAU;
		let min_cos = self.settings.spread_degrees.to_radians().cos();
		let cos_angle = 1.0 - self.random() * (1.0 - min_cos);
		let sin_angle = (1.0 - cos_angle * cos_angle).max(0.0).sqrt();
		let (sin_azimuth, cos_azimuth) = azimuth.sin_cos();
		let direction = Vec3::new(cos_azimuth * sin_angle, cos_angle, sin_azimuth * sin_angle);
		(rotation * direction) * self.settings.speed
	}

	fn random(&mut self) -> f32 {
		self.random_state = self
			.random_state
			.wrapping_mul(1_664_525)
			.wrapping_add(1_013_904_223);
		(self.random_state >> 8) as f32 * (1.0 / 16_777_216.0)
	}
}

/// Iterate emitters in entity-key order so capacity pressure is deterministic.
#[derive(Clone, Default)]
pub(crate) struct ParticleBank {
	emitters: BTreeMap<EntityKey, Emitter>,
}

impl ParticleBank {
	pub(crate) fn new(world: &RuntimeWorld) -> Self {
		let mut bank = Self::default();
		bank.admit(&world.data().entities);
		bank
	}

	pub(crate) fn admit(&mut self, entities: &[EntityData]) {
		for entity in entities {
			if let Some(emitter) = entity
				.particle_emitter
				.as_ref()
				.filter(|emitter| emitter.enabled)
			{
				self.emitters
					.insert(entity.key.clone(), Emitter::new(emitter.settings));
			}
		}
	}

	pub(crate) fn needs_tick(&self) -> bool {
		self.emitters
			.values()
			.any(|emitter| emitter.settings.rate_per_second > 0.0 || !emitter.particles.is_empty())
	}

	pub(crate) fn samples(&self, fixed_hz: u32) -> impl Iterator<Item = ParticleSample> + '_ {
		self.emitters.values().flat_map(move |emitter| {
			emitter
				.particles
				.iter()
				.map(move |particle| particle.sample(&emitter.settings, fixed_hz))
		})
	}

	pub(crate) fn step(&mut self, world: &RuntimeWorld, fixed_hz: u32) -> bool {
		let had_particles = self
			.emitters
			.values()
			.any(|emitter| !emitter.particles.is_empty());
		self.emitters.retain(|key, emitter| {
			emitter.age(fixed_hz);
			world.entity_index(key).is_some()
		});
		let mut active: usize = self
			.emitters
			.values()
			.map(|emitter| emitter.particles.len())
			.sum();
		let mut rotations = BirthRotations::default();
		for (key, emitter) in &mut self.emitters {
			let births = emitter.accepted_births(fixed_hz, MAX_PARTICLES - active);
			if births == 0 {
				continue;
			}
			if let Some(index) = world.entity_index(key) {
				let origin = world.world_matrices()[index].w_axis.truncate();
				emitter.emit(births, origin, rotations.get(world, index));
				active += births;
			}
		}
		had_particles || active > 0
	}
}

/// Cache shared ancestors within this birth step. Authored rotation keeps the cone
/// circular even when an object or ancestor has a nonuniform or zero scale.
#[derive(Default)]
struct BirthRotations {
	resolved: HashMap<usize, Quat>,
	lineage: Vec<usize>,
}

impl BirthRotations {
	fn get(&mut self, world: &RuntimeWorld, index: usize) -> Quat {
		let entities = &world.data().entities;
		let mut next = Some(index);
		let mut rotation = Quat::IDENTITY;
		while let Some(index) = next {
			if let Some(&known) = self.resolved.get(&index) {
				rotation = known;
				break;
			}
			self.lineage.push(index);
			next = entities[index]
				.parent
				.as_ref()
				.and_then(|key| world.entity_index(key));
		}
		for index in self.lineage.drain(..).rev() {
			let [x, y, z] = entities[index].transform.rotation;
			rotation *= Quat::from_euler(EulerRot::XYZ, x, y, z);
			self.resolved.insert(index, rotation);
		}
		rotation.normalize()
	}
}
