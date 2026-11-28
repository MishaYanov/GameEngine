use std::collections::HashMap;

use bevy_mikktspace::{Geometry, TangentSpace};
use glam::{Vec2, Vec3};

#[derive(Clone, Copy)]
pub struct NormalVertex {
	pub position: [f32; 3],
	pub normal: Option<[f32; 3]>,
	pub uv: [f32; 2],
}

#[derive(Clone, Copy)]
pub struct ResolvedTangent {
	pub source: usize,
	pub normal: [f32; 3],
	pub tangent: [f32; 4],
}

struct MikkGeometry<'a> {
	vertices: &'a [NormalVertex],
	indices: &'a [u32],
	normals: &'a [[f32; 3]],
	tangents: Vec<[f32; 4]>,
}

impl MikkGeometry<'_> {
	fn corner(face: usize, vert: usize) -> usize {
		face * 3 + vert
	}

	fn vertex(&self, face: usize, vert: usize) -> NormalVertex {
		self.vertices[self.indices[Self::corner(face, vert)] as usize]
	}
}

impl Geometry for MikkGeometry<'_> {
	fn num_faces(&self) -> usize {
		self.indices.len() / 3
	}

	fn num_vertices_of_face(&self, _: usize) -> usize {
		3
	}

	fn position(&self, face: usize, vert: usize) -> [f32; 3] {
		self.vertex(face, vert).position
	}

	fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
		self.normals[Self::corner(face, vert)]
	}

	fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
		self.vertex(face, vert).uv
	}

	fn set_tangent(&mut self, space: Option<TangentSpace>, face: usize, vert: usize) {
		self.tangents[Self::corner(face, vert)] = space.map_or([0.0; 4], |s| s.tangent_encoded());
	}
}

/// Conservative admission cost for per-corner remapping and MikkTSpace scratch
/// data. Also accounts for a potential duplicate of each output vertex.
pub const GENERATED_TANGENT_BYTES_PER_CORNER: usize = 512;

pub fn validate_normal_texture(
	info: &gltf::material::NormalTexture<'_>,
) -> Result<(), &'static str> {
	use gltf::texture::{MagFilter, MinFilter, WrappingMode};

	if !info.scale().is_finite() || info.scale() < 0.0 {
		return Err("normal texture scale must be finite and nonnegative");
	}
	let sampler = info.texture().sampler();
	if sampler.wrap_s() != WrappingMode::Repeat
		|| sampler.wrap_t() != WrappingMode::Repeat
		|| sampler
		.mag_filter()
		.is_some_and(|value| value != MagFilter::Linear)
		|| sampler.min_filter().is_some_and(|value| {
		!matches!(value, MinFilter::Linear | MinFilter::LinearMipmapLinear)
	}) {
		return Err("normal textures use linear filtering and repeat wrapping");
	}
	Ok(())
}

pub fn report_normal_mip_fallback(info: &gltf::material::NormalTexture<'_>) {
	if info.texture().sampler().min_filter() == Some(gltf::texture::MinFilter::LinearMipmapLinear) {
		engine_logger::warn!(
			"Normal texture {} requests trilinear mipmaps; using linear filtering at the base level",
			info.texture().index()
		);
	}
}

pub fn valid_normal_uv(accessor: &gltf::Accessor<'_>) -> bool {
	use gltf::accessor::{DataType, Dimensions};

	accessor.dimensions() == Dimensions::Vec2
		&& matches!(
			(accessor.data_type(), accessor.normalized()),
			(DataType::F32, false) | (DataType::U8 | DataType::U16, true)
		)
}

pub fn normal_tangent(tangent: [f32; 4], normal: [f32; 3]) -> [f32; 4] {
	let axis = Vec3::from_array([tangent[0], tangent[1], tangent[2]]);
	let normal = Vec3::from_array(normal);
	if !axis.is_finite() || tangent[3].abs() != 1.0 {
		return [0.0; 4];
	}
	let Some(axis) = (axis - normal * normal.dot(axis)).try_normalize() else {
		return [0.0; 4];
	};
	[axis.x, axis.y, axis.z, tangent[3]]
}

/// Generate per-corner tangent frames, retaining original indices for the first
/// encountered frame at a vertex and splitting only incompatible corners.
/// The caller bounds scratch space and validates indices and finite inputs.
pub fn generate(vertices: &[NormalVertex], indices: &mut [u32]) -> Vec<ResolvedTangent> {
	let mut normals = Vec::with_capacity(indices.len());
	let mut usable = Vec::with_capacity(indices.len() / 3);
	for face in indices.chunks_exact(3) {
		let [a, b, c] = std::array::from_fn(|i| vertices[face[i] as usize]);
		let point = Vec3::from_array(a.position);
		let cross =
			(Vec3::from_array(b.position) - point).cross(Vec3::from_array(c.position) - point);
		let flat = cross.try_normalize();
		let origin = Vec2::from_array(a.uv);
		let uv_area = (Vec2::from_array(b.uv) - origin).perp_dot(Vec2::from_array(c.uv) - origin);
		usable.push(flat.is_some() && uv_area.is_finite() && uv_area != 0.0);
		let flat = flat.unwrap_or(Vec3::Z).to_array();
		normals.extend([a, b, c].map(|vertex| vertex.normal.unwrap_or(flat)));
	}
	let mut geometry = MikkGeometry {
		vertices,
		indices,
		normals: &normals,
		tangents: vec![[0.0; 4]; indices.len()],
	};
	if usable.iter().any(|&valid| valid)
		&& bevy_mikktspace::generate_tangents(&mut geometry).is_err()
	{
		geometry.tangents.fill([0.0; 4]);
	}
	let tangents = geometry.tangents;
	let mut resolved: Vec<_> = vertices
		.iter()
		.enumerate()
		.map(|(source, vertex)| ResolvedTangent {
			source,
			normal: vertex.normal.unwrap_or([0., 0., 1.]),
			tangent: [0.0; 4],
		})
		.collect();
	let mut assigned = vec![false; vertices.len()];
	let mut frames = HashMap::new();
	for (corner, index) in indices.iter_mut().enumerate() {
		let source = *index as usize;
		let normal = normals[corner];
		let tangent = if usable[corner / 3] {
			normal_tangent(tangents[corner], normal)
		} else {
			[0.0; 4]
		};
		let vertex = ResolvedTangent {
			source,
			normal,
			tangent,
		};
		// Canonicalize signed zero to keep equivalent corners welded.
		let bits = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
		*index = *frames
			.entry((source, normal.map(bits), tangent.map(bits)))
			.or_insert_with(|| {
				if assigned[source] {
					let next = resolved.len() as u32;
					resolved.push(vertex);
					next
				} else {
					assigned[source] = true;
					resolved[source] = vertex;
					source as u32
				}
			});
	}
	resolved
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn generates_oriented_tangents_and_splits_mirrored_corners() {
		let vertex = |position, uv| NormalVertex {
			position,
			normal: Some([0., 0., 1.]),
			uv,
		};
		let vertices = [
			vertex([0., 0., 0.], [0., 0.]),
			vertex([1., 0., 0.], [1., 0.]),
			vertex([0., 1., 0.], [0., 1.]),
			vertex([-1., 0., 0.], [1., 0.]),
		];
		let mut indices = [0, 1, 2, 0, 2, 3];
		let mapped = generate(&vertices, &mut indices);
		assert_ne!(indices[0], indices[3]);
		assert_eq!(mapped[indices[0] as usize].tangent, [1., 0., 0., 1.]);
		assert_eq!(mapped[indices[3] as usize].tangent, [-1., 0., 0., -1.]);
	}

	#[test]
	fn leaves_degenerate_uvs_on_flat_geometric_normals() {
		let vertex = |position| NormalVertex {
			position,
			normal: None,
			uv: [0.; 2],
		};
		let mut indices = [0, 1, 2];
		let output = generate(
			&[
				vertex([0., 0., 0.]),
				vertex([1., 0., 0.]),
				vertex([0., 0., 1.]),
			],
			&mut indices,
		);
		for corner in output {
			assert_eq!(corner.normal, [0., -1., 0.]);
			assert_eq!(corner.tangent, [0.; 4]);
		}
	}
}
