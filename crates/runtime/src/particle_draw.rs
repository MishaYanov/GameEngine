//! Draw committed particles as opaque, depth-tested, camera-facing diamonds.

use engine_core::particles::MAX_PARTICLES;
pub use engine_core::particles::ParticleSample;
use glam::{Mat4, Vec3};
use renderer::{
	WindowRenderer,
	vulkan::{CameraUniform, ModelPushConstants, RenderObject, Vertex, VulkanMaterial, VulkanMesh},
};
use std::sync::Arc;

const VERTICES_PER_PARTICLE: usize = 4;
const INDICES_PER_PARTICLE: usize = 6;
// The world shader uses the normal attribute as RGB when this flag is set.
const VERTEX_COLOR: u32 = 4;

pub struct ParticleDraw {
	mesh: VulkanMesh,
	material: VulkanMaterial,
	vertices: Vec<Vertex>,
	indices: Vec<u32>,
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
			material: VulkanMaterial::new(Arc::new(white), true, [1.0; 4]),
			vertices: Vec::new(),
			indices: Vec::new(),
		})
	}

	/// Allocate GPU resources only when a drawable particle frame first arrives.
	pub fn prepare(
		draw: &mut Option<Self>,
		renderer: &WindowRenderer,
		camera: &CameraUniform,
		samples: &[ParticleSample],
	) -> Result<(), String> {
		if samples.is_empty() {
			if let Some(draw) = draw {
				draw.vertices.clear();
				draw.indices.clear();
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
		billboard_geometry(
			Mat4::from_cols_array(&camera.view_projection),
			samples,
			&mut draw.vertices,
			&mut draw.indices,
		);
		renderer.write_dynamic_mesh(&mut draw.mesh, &draw.vertices, &draw.indices)
	}

	pub fn object(&self) -> Option<RenderObject<'_>> {
		(!self.indices.is_empty()).then(|| {
			let mut constants = ModelPushConstants::identity();
			constants.flags[0] = VERTEX_COLOR;
			RenderObject::new(&self.mesh, &self.material, constants)
		})
	}
}

fn billboard_geometry(
	view_projection: Mat4,
	samples: &[ParticleSample],
	vertices: &mut Vec<Vertex>,
	indices: &mut Vec<u32>,
) {
	vertices.clear();
	indices.clear();
	vertices.reserve(samples.len() * VERTICES_PER_PARTICLE);
	indices.reserve(samples.len() * INDICES_PER_PARTICLE);

	let clip_space = view_projection.transpose();
	let right = clip_space.x_axis.truncate().normalize_or_zero();
	let up = clip_space.y_axis.truncate().normalize_or_zero();
	for sample in samples {
		let base = vertices.len() as u32;
		let center = Vec3::from_array(sample.position);
		let radius = sample.size * 0.5;
		for axis in [-right, -up, right, up] {
			vertices.push(
				Vertex::new((center + radius * axis).to_array(), [0.5; 2])
					.with_normal(sample.color),
			);
		}
		indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn particles_use_camera_facing_geometry_and_rgb() {
		let camera =
			CameraUniform::perspective([1., 0., 0.], [0.; 3], [0., 1., 0.], 1.0, 2.0, 0.1, 50.0);
		let camera = Mat4::from_cols_array(&camera.view_projection);
		let particle = ParticleSample {
			position: [2., 3., 4.],
			size: 2.,
			color: [0.9, 0.2, 0.1],
		};
		let mut vertices = Vec::new();
		let mut indices = Vec::new();
		billboard_geometry(camera, &[particle, particle], &mut vertices, &mut indices);
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

		billboard_geometry(camera, &[], &mut vertices, &mut indices);
		assert!(vertices.is_empty() && indices.is_empty());
	}
}
