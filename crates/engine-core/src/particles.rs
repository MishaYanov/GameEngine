//! Committed world-space particle presentation, independent of renderer resources.

use crate::{
	EntityKey,
	asset_catalog::AssetId,
	components::{
		MAX_ENABLED_PARTICLE_EMITTERS, MAX_PARTICLES_PER_EMITTER, ParticleBurstSettings,
		ParticleEmitterSettings,
	},
	scene_data::EntityData,
	world::RuntimeWorld,
};
use glam::{EulerRot, Quat, Vec3};
use std::{
	collections::{BTreeMap, HashMap},
	sync::Arc,
};

pub const MAX_PARTICLES: usize = 2048;

/// A renderer-independent sample. Size is the billboard diameter in world units.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSample {
	pub position: [f32; 3],
	pub size: f32,
	pub color: [f32; 3],
	pub sprite_asset_id: Option<Arc<AssetId>>,
}

#[derive(Clone)]
struct Particle {
	origin: Vec3,
	velocity: Vec3,
	age_steps: u32,
}

impl Particle {
	fn sample(&self, emitter: &Emitter, fixed_hz: u32) -> ParticleSample {
		let settings = &emitter.settings;
		let age = self.age_steps as f32 / fixed_hz as f32;
		let normalized_age = (age / settings.lifetime_seconds).min(1.0);
		let acceleration = Vec3::Y * (0.5 * settings.gravity * age * age);
		ParticleSample {
			position: (self.origin + self.velocity * age + acceleration).to_array(),
			size: settings.size * (1.0 - 0.75 * normalized_age),
			color: settings.color,
			sprite_asset_id: emitter.sprite_asset_id.clone(),
		}
	}
}

#[derive(Clone)]
struct Emitter {
	settings: ParticleEmitterSettings,
	sprite_asset_id: Option<Arc<AssetId>>,
	random_state: u32,
	birth_credit: f64,
	particles: Vec<Particle>,
}

impl Emitter {
	fn new(settings: &ParticleEmitterSettings) -> Self {
		Self {
			random_state: settings.seed,
			birth_credit: 0.0,
			settings: settings.clone(),
			sprite_asset_id: settings.sprite_asset_id.clone().map(Arc::new),
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
	retired: Vec<Emitter>,
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
					.insert(entity.key.clone(), Emitter::new(&emitter.settings));
			}
		}
	}

	pub(crate) fn sync_runtime_emitter(&mut self, entity: &EntityData) -> Result<bool, String> {
		let emitter = entity.particle_emitter.as_ref().expect("verified emitter");
		if emitter.enabled {
			if self.emitters.len() >= MAX_ENABLED_PARTICLE_EMITTERS {
				return Err(format!(
					"Scene exceeds {MAX_ENABLED_PARTICLE_EMITTERS} enabled Particle Emitters"
				));
			}
			self.admit(std::slice::from_ref(entity));
			return Ok(false);
		}
		Ok(self
			.emitters
			.remove(&entity.key)
			.is_some_and(|emitter| !emitter.particles.is_empty()))
	}

	pub(crate) fn needs_tick(&self) -> bool {
		!self.retired.is_empty()
			|| self.emitters.values().any(|emitter| {
			emitter.settings.rate_per_second > 0.0 || !emitter.particles.is_empty()
		})
	}

	pub(crate) fn samples(&self, fixed_hz: u32) -> impl Iterator<Item=ParticleSample> + '_ {
		self.emitters
			.values()
			.chain(&self.retired)
			.flat_map(move |emitter| {
				emitter
					.particles
					.iter()
					.map(move |particle| particle.sample(emitter, fixed_hz))
			})
	}

	/// Emit once in world space. Surviving particles count against the scene budget.
	pub(crate) fn burst(
		&mut self,
		position: [f32; 3],
		burst: &ParticleBurstSettings,
	) -> Result<bool, String> {
		burst.validate()?;
		let origin = Vec3::from_array(position);
		if !origin.is_finite() {
			return Err("Particle burst position must be finite".into());
		}
		let count = usize::from(burst.count).min(MAX_PARTICLES - self.particle_count());
		if count == 0 {
			return Ok(false);
		}
		let mut emitter = Emitter::new(&burst.settings);
		emitter.particles.reserve_exact(count);
		emitter.emit(count, origin, Quat::IDENTITY);
		self.retired.push(emitter);
		Ok(true)
	}

	fn particle_count(&self) -> usize {
		self.emitters
			.values()
			.chain(&self.retired)
			.map(|emitter| emitter.particles.len())
			.sum()
	}

	pub(crate) fn step(&mut self, world: &RuntimeWorld, fixed_hz: u32) -> bool {
		let had_particles = !self.retired.is_empty()
			|| self
			.emitters
			.values()
			.any(|emitter| !emitter.particles.is_empty());
		self.retired.retain_mut(|emitter| {
			emitter.age(fixed_hz);
			!emitter.particles.is_empty()
		});
		for (_, mut emitter) in self.emitters.extract_if(.., |key, emitter| {
			emitter.age(fixed_hz);
			world.entity_index(key).is_none()
		}) {
			if emitter.settings.linger_on_despawn && !emitter.particles.is_empty() {
				// Retired groups never add births. Keep their motion/texture settings.
				emitter.birth_credit = 0.0;
				self.retired.push(emitter);
			}
		}
		let mut active = self.particle_count();
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
