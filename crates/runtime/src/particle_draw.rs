//! Draw committed particles as depth-tested diamonds or alpha-cutout PNG billboards.

pub use engine_core::particles::ParticleSample;
use engine_core::{asset_catalog::AssetId, particles::MAX_PARTICLES};
use glam::{Mat4, Vec3};
use renderer::{
	WindowRenderer,
	vulkan::{CameraUniform, ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use std::{ops::Range, sync::Arc};

use crate::assets::{AssetManager, ImageAsset};

const VERTICES_PER_PARTICLE: usize = 4;
const INDICES_PER_PARTICLE: usize = 6;
// The world shader uses the normal attribute as RGB when this flag is set.
const VERTEX_COLOR: u32 = 4;
const SPRITE_CUTOUT: u32 = 256;

pub struct ParticleDraw {
	mesh: VulkanMesh,
	white: VulkanMaterial,
	vertices: Vec<Vertex>,
	indices: Vec<u32>,
	order: Vec<usize>,
	batches: Vec<SpriteBatch>,
}

struct SpriteBatch {
	indices: Range<u32>,
	material: Option<VulkanMaterial>,
}

impl ParticleDraw {
	fn new(renderer: &WindowRenderer) -> Result<Self, String> {
		let white = renderer
			.upload_rgba8(1, 1, &[255; 4])
			.map_err(|error| error.to_string())?;
		Ok(Self {
			mesh: renderer.create_dynamic_mesh(
				VERTICES_PER_PARTICLE * MAX_PARTICLES,
				INDICES_PER_PARTICLE * MAX_PARTICLES,
			)?,
			white: VulkanMaterial::new(Arc::new(white), true, [1.0; 4]),
			vertices: Vec::new(),
			indices: Vec::new(),
			order: Vec::new(),
			batches: Vec::new(),
		})
	}

	/// Allocate GPU resources only when a drawable particle frame first arrives.
	pub fn prepare(
		draw: &mut Option<Self>,
		renderer: &WindowRenderer,
		camera: &CameraUniform,
		samples: &[ParticleSample],
		mut assets: Option<&mut AssetManager>,
	) -> Result<(), String> {
		if samples.is_empty() {
			if let Some(draw) = draw {
				draw.vertices.clear();
				draw.indices.clear();
				draw.batches.clear();
			}
			return Ok(());
		}
		if samples.len() > MAX_PARTICLES {
			return Err("Particle frame exceeds its sample limit".into());
		}
		if draw.is_none() {
			*draw = Some(Self::new(renderer)?);
		}
		let draw = draw.as_mut().expect("particle GPU resources prepared");
		draw.vertices.clear();
		draw.vertices.reserve(samples.len() * VERTICES_PER_PARTICLE);
		draw.indices.clear();
		draw.indices.reserve(samples.len() * INDICES_PER_PARTICLE);
		draw.batches.clear();
		draw.order.clear();
		draw.order.extend(0..samples.len());
		draw.order.sort_unstable_by(|&a, &b| {
			samples[a]
				.sprite_asset_id
				.cmp(&samples[b].sprite_asset_id)
				.then_with(|| a.cmp(&b))
		});
		let view = Mat4::from_cols_array(&camera.view_projection).transpose();
		let right = view.x_axis.truncate().normalize_or_zero();
		let up = view.y_axis.truncate().normalize_or_zero();
		for group in draw
			.order
			.chunk_by(|&a, &b| samples[a].sprite_asset_id == samples[b].sprite_asset_id)
		{
			let material = if let Some(id) = &samples[group[0]].sprite_asset_id {
				let manager = assets
					.as_deref_mut()
					.ok_or("Particle sprites require project assets")?;
				Some(load_sprite_material(manager, renderer, id)?)
			} else {
				None
			};
			let start = draw.indices.len() as u32;
			for &index in group {
				push_billboard(
					right,
					up,
					&samples[index],
					&mut draw.vertices,
					&mut draw.indices,
				);
			}
			draw.batches.push(SpriteBatch {
				indices: start..draw.indices.len() as u32,
				material,
			});
		}
		renderer.write_dynamic_mesh(&mut draw.mesh, &draw.vertices, &draw.indices)
	}

	pub fn objects(&self) -> impl Iterator<Item=RenderObject<'_>> {
		self.batches.iter().filter_map(|batch| {
			let mut constants = ModelPushConstants::identity();
			constants.flags[0] = VERTEX_COLOR;
			let material = match &batch.material {
				Some(material) => {
					constants.flags[0] |= SPRITE_CUTOUT;
					material
				}
				None => &self.white,
			};
			RenderObject::new(&self.mesh, material, constants)
				.with_index_range(batch.indices.clone())
		})
	}
}

fn load_sprite_material(
	assets: &mut AssetManager,
	renderer: &WindowRenderer,
	id: &AssetId,
) -> Result<VulkanMaterial, String> {
	let key = format!("particle-sprite:{}", id.as_str());
	let texture = match assets.cached_image_texture(&key) {
		Some(texture) => texture,
		None => {
			let (_, sprite) = bridge::particle_sprite::read_project(assets.root(), id)?;
			let image = ImageAsset::from_rgba8(sprite.width, sprite.height, sprite.rgba8)
				.map_err(|error| error.to_string())?;
			assets
				.load_image_texture(renderer, key, &image)
				.map_err(|error| error.to_string())?
		}
	};
	Ok(VulkanMaterial::new(texture, true, [1.0; 4]))
}

fn push_billboard(
	right: Vec3,
	up: Vec3,
	sample: &ParticleSample,
	vertices: &mut Vec<Vertex>,
	indices: &mut Vec<u32>,
) {
	let base = vertices.len() as u32;
	let center = Vec3::from_array(sample.position);
	let radius = sample.size * 0.5;
	let corners = if sample.sprite_asset_id.is_some() {
		// Vulkan's positive viewport height maps the smaller clip Y to the top row.
		[
			(-right - up, [0.0, 0.0]),
			(-right + up, [0.0, 1.0]),
			(right + up, [1.0, 1.0]),
			(right - up, [1.0, 0.0]),
		]
	} else {
		[
			(-right, [0.5; 2]),
			(-up, [0.5; 2]),
			(right, [0.5; 2]),
			(up, [0.5; 2]),
		]
	};
	for (axis, uv) in corners {
		vertices
			.push(Vertex::new((center + radius * axis).to_array(), uv).with_normal(sample.color));
	}
	indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn particles_use_camera_facing_geometry_and_rgb() {
		let camera =
			CameraUniform::perspective([1., 0., 0.], [0.; 3], [0., 1., 0.], 1.0, 2.0, 0.1, 50.0);
		let camera = Mat4::from_cols_array(&camera.view_projection);
		let clip_space = camera.transpose();
		let right = clip_space.x_axis.truncate().normalize_or_zero();
		let up = clip_space.y_axis.truncate().normalize_or_zero();
		let particle = ParticleSample {
			position: [2., 3., 4.],
			size: 2.,
			color: [0.9, 0.2, 0.1],
			sprite_asset_id: None,
		};
		let mut vertices = Vec::new();
		let mut indices = Vec::new();
		push_billboard(right, up, &particle, &mut vertices, &mut indices);
		push_billboard(right, up, &particle, &mut vertices, &mut indices);
		let expected = [[2., 3., 5.], [2., 4., 4.], [2., 3., 3.], [2., 2., 4.]];
		for (vertex, position) in vertices.iter().zip(expected) {
			assert!(
				Vec3::from_array(vertex.position).abs_diff_eq(Vec3::from_array(position), 0.000_01)
			);
		}
		assert!(
			vertices
				.iter()
				.all(|vertex| vertex.normal == particle.color)
		);
		assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);

		vertices.clear();
		indices.clear();
		let sprite = ParticleSample {
			sprite_asset_id: Some(Arc::new(AssetId::generate())),
			..particle
		};
		push_billboard(right, up, &sprite, &mut vertices, &mut indices);
		assert_eq!(
			vertices.iter().map(|vertex| vertex.uv).collect::<Vec<_>>(),
			vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
		);
		let clip_y =
			|index: usize| (camera * Vec3::from_array(vertices[index].position).extend(1.0)).y;
		assert!(clip_y(0) < clip_y(1) && clip_y(3) < clip_y(2));
	}
}
