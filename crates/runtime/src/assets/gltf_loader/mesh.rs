use std::collections::HashSet;

use bridge::normal_tangents::{self, GENERATED_TANGENT_BYTES_PER_CORNER, NormalVertex};
use gltf::{
	accessor::{DataType, Dimensions},
	mesh::Mode,
};
use renderer::vulkan::Vertex;

use super::{
	GltfLoaderError, IMPORT_LIMIT, ImportedNormalMap, ImportedPrimitive, charge, rejected,
	resources::ImageResource,
};
use crate::assets::{MaterialData, MeshData};

pub(super) fn import_first_mesh(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	if document.meshes().next().is_none() {
		return Err(GltfLoaderError::NoMeshes);
	}
	import_mesh(document, buffers, images, 0)
}

pub(super) fn import_mesh(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
	mesh_index: usize,
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	import_mesh_checked(document, buffers, images, mesh_index, &|| Ok(()))
}

pub(super) fn import_mesh_checked(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
	mesh_index: usize,
	checkpoint: &impl Fn() -> Result<(), GltfLoaderError>,
) -> Result<Vec<ImportedPrimitive>, GltfLoaderError> {
	checkpoint()?;
	preflight_meshes(document, [mesh_index])?;
	let mesh = document
		.meshes()
		.nth(mesh_index)
		.ok_or_else(|| rejected(format!("mesh index {mesh_index} does not exist")))?;
	if mesh.primitives().len() > 4096 {
		return Err(rejected("mesh exceeds 4096 primitives"));
	}
	let mut parts = Vec::with_capacity(mesh.primitives().len());
	let mut budget = IMPORT_LIMIT;
	let mut shared_images = HashSet::new();
	for primitive in mesh.primitives() {
		checkpoint()?;
		// Accessor counts are checked before allocating decoded vertex buffers.
		let vertices = primitive
			.get(&gltf::Semantic::Positions)
			.map_or(0, |a| a.count());
		let indices = primitive.indices().map_or(vertices, |a| a.count());
		let vertex_bytes = vertices
			.checked_mul(std::mem::size_of::<Vertex>() + 48)
			.ok_or_else(|| rejected("geometry size overflow"))?;
		let index_bytes = indices
			.checked_mul(std::mem::size_of::<u32>())
			.ok_or_else(|| rejected("geometry size overflow"))?;
		charge(
			&mut budget,
			vertex_bytes
				.checked_add(index_bytes)
				.ok_or_else(|| rejected("geometry size overflow"))?,
		)?;
		charge(
			&mut budget,
			normal_geometry_bytes(&primitive, vertices, indices, mesh.index())?,
		)?;
		parts.push(import_primitive(
			mesh.index(),
			primitive,
			buffers,
			images,
			&mut shared_images,
			&mut budget,
		)?);
	}
	if parts.is_empty() {
		return Err(GltfLoaderError::NoPrimitives);
	}
	Ok(parts)
}

/// Admit only contiguous or strided ordinary accessor views for warm decoding.
/// Library iterators require the final element to fit both the view and buffer.
pub(super) fn validate_warm_accessor_ranges(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	mesh_index: usize,
	checkpoint: &impl Fn() -> Result<(), GltfLoaderError>,
) -> Result<(), GltfLoaderError> {
	let mesh = document
		.meshes()
		.nth(mesh_index)
		.ok_or_else(|| rejected(format!("mesh index {mesh_index} does not exist")))?;
	for primitive in mesh.primitives() {
		checkpoint()?;
		let uv = primitive
			.material()
			.pbr_metallic_roughness()
			.base_color_texture()
			.map_or(0, |texture| texture.tex_coord());
		let normal_uv = primitive
			.material()
			.normal_texture()
			.map(|info| info.tex_coord());
		for (accessor, types, dimensions) in [
			(
				primitive.get(&gltf::Semantic::Positions),
				&[DataType::F32][..],
				Dimensions::Vec3,
			),
			(
				primitive.get(&gltf::Semantic::Normals),
				&[DataType::F32][..],
				Dimensions::Vec3,
			),
			(
				primitive.get(&gltf::Semantic::TexCoords(uv)),
				&[DataType::U8, DataType::U16, DataType::F32][..],
				Dimensions::Vec2,
			),
			(
				normal_uv.and_then(|uv| primitive.get(&gltf::Semantic::TexCoords(uv))),
				&[DataType::U8, DataType::U16, DataType::F32][..],
				Dimensions::Vec2,
			),
			(
				normal_uv.and_then(|_| primitive.get(&gltf::Semantic::Tangents)),
				&[DataType::F32][..],
				Dimensions::Vec4,
			),
			(
				primitive.indices(),
				&[DataType::U8, DataType::U16, DataType::U32][..],
				Dimensions::Scalar,
			),
		] {
			let Some(accessor) = accessor else { continue };
			if accessor.sparse().is_some()
				|| accessor.dimensions() != dimensions
				|| !types.contains(&accessor.data_type())
			{
				return Err(rejected(
					"model warming defers unsupported accessor layouts",
				));
			}
			let view = accessor
				.view()
				.ok_or_else(|| rejected("warm accessor needs a buffer view"))?;
			let buffer = buffers
				.get(view.buffer().index())
				.ok_or_else(|| rejected("warm accessor buffer is missing"))?;
			let size = accessor.size();
			let stride = view.stride().unwrap_or(size);
			let end = accessor
				.count()
				.checked_sub(1)
				.and_then(|count| count.checked_mul(stride))
				.and_then(|remaining| accessor.offset().checked_add(remaining))
				.and_then(|offset| offset.checked_add(size));
			let view_end = view.offset().checked_add(view.length());
			if stride < size
				|| end.is_none_or(|end| end > view.length())
				|| view_end.is_none_or(|end| end > buffer.0.len())
			{
				return Err(rejected("warm accessor exceeds its buffer view"));
			}
		}
	}
	Ok(())
}

/// Charge every decoded primitive once per selected mesh. Shared accessors
/// still expand into separate CPU/GPU primitive arrays and must be charged
/// again; repeated scene nodes referencing the same mesh are deduplicated.
pub(in crate::assets) fn preflight_meshes(
	document: &gltf::Document,
	mesh_indices: impl IntoIterator<Item = usize>,
) -> Result<usize, GltfLoaderError> {
	let mut selected = vec![false; document.meshes().len()];
	for index in mesh_indices {
		*selected
			.get_mut(index)
			.ok_or_else(|| rejected(format!("mesh index {index} does not exist")))? = true;
	}
	let mut budget = IMPORT_LIMIT;
	for mesh in document.meshes().filter(|mesh| selected[mesh.index()]) {
		if mesh.primitives().len() > 4096 {
			return Err(rejected("mesh exceeds 4096 primitives"));
		}
		for primitive in mesh.primitives() {
			let vertices = primitive
				.get(&gltf::Semantic::Positions)
				.ok_or(GltfLoaderError::MissingPositions)?
				.count();
			let indices = primitive
				.indices()
				.map_or(vertices, |accessor| accessor.count());
			if vertices == 0 || indices == 0 {
				return Err(GltfLoaderError::EmptyGeometry);
			}
			if primitive
				.get(&gltf::Semantic::Normals)
				.is_some_and(|accessor| accessor.count() != vertices)
			{
				return Err(rejected("NORMAL count must match POSITION before decoding"));
			}
			let uv_set = primitive
				.material()
				.pbr_metallic_roughness()
				.base_color_texture()
				.map_or(0, |texture| texture.tex_coord());
			if let Some(accessor) = primitive.get(&gltf::Semantic::TexCoords(uv_set))
				&& accessor.count() != vertices
			{
				return Err(GltfLoaderError::TexCoordCountMismatch {
					positions: vertices,
					tex_coords: accessor.count(),
				});
			}
			let bytes = vertices
				.checked_mul(std::mem::size_of::<Vertex>() + 48)
				.and_then(|bytes| {
					indices
						.checked_mul(std::mem::size_of::<u32>())
						.and_then(|indices| bytes.checked_add(indices))
				})
				.ok_or_else(|| rejected("decoded geometry size overflow"))?;
			charge(&mut budget, bytes)?;
			charge(
				&mut budget,
				normal_geometry_bytes(&primitive, vertices, indices, mesh.index())?,
			)?;
		}
	}
	Ok(IMPORT_LIMIT - budget)
}

fn normal_geometry_bytes(
	primitive: &gltf::Primitive<'_>,
	vertices: usize,
	indices: usize,
	mesh: usize,
) -> Result<usize, GltfLoaderError> {
	let Some(info) = primitive.material().normal_texture() else {
		return Ok(0);
	};
	let invalid = |message| {
		rejected(format!(
			"mesh {mesh}, primitive {}: {message}",
			primitive.index()
		))
	};
	normal_tangents::validate_normal_texture(&info).map_err(invalid)?;
	let uv = primitive.get(&gltf::Semantic::TexCoords(info.tex_coord()));
	if uv.is_none_or(|uv| uv.count() != vertices || !normal_tangents::valid_normal_uv(&uv)) {
		return Err(invalid(
			"normal texture requires a valid declared TEXCOORD set with the POSITION count",
		));
	}
	let tangent = primitive.get(&gltf::Semantic::Tangents);
	if tangent.as_ref().is_some_and(|value| {
		value.count() != vertices
			|| value.dimensions() != Dimensions::Vec4
			|| value.data_type() != DataType::F32
			|| value.normalized()
	}) {
		return Err(invalid(
			"TANGENT requires one non-normalized float VEC4 per POSITION",
		));
	}
	let bytes = vertices.checked_mul(64).and_then(|bytes| {
		if tangent.is_none() || primitive.get(&gltf::Semantic::Normals).is_none() {
			bytes.checked_add(indices.checked_mul(GENERATED_TANGENT_BYTES_PER_CORNER)?)
		} else {
			Some(bytes)
		}
	});
	bytes.ok_or_else(|| invalid("normal geometry size overflow"))
}

fn import_primitive(
	mesh_index: usize,
	primitive: gltf::Primitive<'_>,
	buffers: &[gltf::buffer::Data],
	images: &[ImageResource],
	shared_images: &mut HashSet<usize>,
	budget: &mut usize,
) -> Result<ImportedPrimitive, GltfLoaderError> {
	if primitive.mode() != Mode::Triangles {
		return Err(GltfLoaderError::UnsupportedMode(primitive.mode()));
	}
	if primitive.morph_targets().next().is_some() {
		return Err(rejected(
			"morph-target primitives are not supported by static mesh import",
		));
	}

	let primitive_index = primitive.index();

	let material = primitive.material();
	let pbr = material.pbr_metallic_roughness();

	let base_color_factor = pbr.base_color_factor();
	let base_color_texture = pbr.base_color_texture();

	let tex_coord_set = base_color_texture
		.as_ref()
		.map(|info| info.tex_coord())
		.unwrap_or(0);

	engine_logger::debug!(
		"Primitive {}: color factor {:?}, double-sided {}",
		primitive_index,
		base_color_factor,
		material.double_sided()
	);

	let base_color_image_index = base_color_texture
		.as_ref()
		.map(|info| info.texture().source().index());

	let base_color_image = match base_color_texture {
		Some(info) => {
			let image_index = info.texture().source().index();
			let Some(ImageResource::Decoded(image)) = images.get(image_index) else {
				return Err(GltfLoaderError::InvalidImageReference(image_index));
			};
			if shared_images.insert(image_index) {
				// Preserve the per-mesh geometry/image budget even though pixels
				// now share the file cache's allocation across meshes.
				charge(budget, image.pixels().len())?;
			}

			engine_logger::debug!(
				"Primitive {}: image {}, {}x{}, {} RGBA bytes, UV set {}",
				primitive_index,
				image_index,
				image.width(),
				image.height(),
				image.pixels().len(),
				tex_coord_set,
			);

			Some(image.clone())
		}

		None => {
			engine_logger::debug!("Primitive {} uses base-color factor only", primitive_index);

			None
		}
	};

	let reader =
		primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));

	let positions: Vec<[f32; 3]> = reader
		.read_positions()
		.ok_or(GltfLoaderError::MissingPositions)?
		.collect();

	let mut indices: Vec<u32> = match reader.read_indices() {
		Some(indices) => indices.into_u32().collect(),
		None => {
			let count = u32::try_from(positions.len())
				.map_err(|_| rejected("mesh exceeds u32 vertex indices"))?;
			(0..count).collect()
		}
	};

	if positions.is_empty() || indices.is_empty() {
		return Err(GltfLoaderError::EmptyGeometry);
	}

	let tex_coords: Vec<[f32; 2]> = match reader.read_tex_coords(tex_coord_set) {
		Some(values) => values.into_f32().collect(),

		None if base_color_image.is_some() => {
			return Err(GltfLoaderError::MissingTexCoords(tex_coord_set));
		}

		None => vec![[0.0, 0.0]; positions.len()],
	};

	if positions.len() != tex_coords.len() {
		return Err(GltfLoaderError::TexCoordCountMismatch {
			positions: positions.len(),
			tex_coords: tex_coords.len(),
		});
	}
	if positions
		.iter()
		.flatten()
		.chain(tex_coords.iter().flatten())
		.any(|value| !value.is_finite())
	{
		return Err(rejected("positions and texture coordinates must be finite"));
	}

	if !indices.len().is_multiple_of(3) {
		return Err(GltfLoaderError::InvalidTriangleIndexCount(indices.len()));
	}

	for &index in &indices {
		let is_in_bounds = usize::try_from(index)
			.map(|index| index < positions.len())
			.unwrap_or(false);

		if !is_in_bounds {
			return Err(GltfLoaderError::IndexOutOfBounds {
				index,
				vertex_count: positions.len(),
			});
		}
	}

	let normals = reader
		.read_normals()
		.map(|values| values.collect::<Vec<_>>());
	if let Some(normals) = &normals
		&& (normals.len() != positions.len()
			|| normals.iter().any(|normal| {
				let normal = glam::Vec3::from_array(*normal);
				!normal.is_finite()
					|| !normal.length_squared().is_finite()
					|| normal.length_squared() < 1.0e-12
			})) {
		return Err(rejected(
			"NORMAL must contain one finite nonzero vector per position",
		));
	}
	let mut vertices: Vec<Vertex> = positions
		.into_iter()
		.zip(tex_coords)
		.enumerate()
		.map(|(index, (position, uv))| {
			Vertex::new(position, uv).with_normal(normals.as_ref().map_or([0.0; 3], |normals| {
				glam::Vec3::from_array(normals[index])
					.normalize()
					.to_array()
			}))
		})
		.collect();
	let normal_map = if let Some(info) = material.normal_texture() {
		normal_tangents::report_normal_mip_fallback(&info);
		let image_index = info.texture().source().index();
		let Some(ImageResource::Decoded(image)) = images.get(image_index) else {
			return Err(GltfLoaderError::InvalidImageReference(image_index));
		};
		if shared_images.insert(image_index) {
			charge(budget, image.pixels().len())?;
		}
		let normal_uvs: Vec<_> = reader
			.read_tex_coords(info.tex_coord())
			.ok_or_else(|| {
				rejected(format!(
					"mesh {mesh_index}, primitive {primitive_index}: missing normal TEXCOORD_{}",
					info.tex_coord()
				))
			})?
			.into_f32()
			.collect();
		if normal_uvs.len() != vertices.len() || normal_uvs.iter().flatten().any(|v| !v.is_finite())
		{
			return Err(rejected(format!(
				"mesh {mesh_index}, primitive {primitive_index}: invalid normal texture coordinates"
			)));
		}
		match reader.read_tangents().filter(|_| normals.is_some()) {
			Some(tangents) => {
				for ((vertex, uv), tangent) in vertices.iter_mut().zip(normal_uvs).zip(tangents) {
					if tangent.into_iter().any(|v| !v.is_finite()) || tangent[3].abs() != 1.0 {
						return Err(rejected(format!(
							"mesh {mesh_index}, primitive {primitive_index}: nonfinite TANGENT or invalid handedness"
						)));
					}
					*vertex = vertex.with_normal_map(
						normal_tangents::normal_tangent(tangent, vertex.normal),
						uv,
					);
				}
			}
			None => {
				let geometry: Vec<_> = vertices
					.iter()
					.zip(&normal_uvs)
					.map(|(vertex, &uv)| NormalVertex {
						position: vertex.position,
						normal: normals.as_ref().map(|_| vertex.normal),
						uv,
					})
					.collect();
				vertices = normal_tangents::generate(&geometry, &mut indices)
					.into_iter()
					.map(|frame| {
						vertices[frame.source]
							.with_normal(frame.normal)
							.with_normal_map(frame.tangent, normal_uvs[frame.source])
					})
					.collect();
			}
		}
		Some(ImportedNormalMap {
			image_index,
			image: image.clone(),
			scale: info.scale(),
		})
	} else {
		None
	};

	engine_logger::debug!(
		"Imported primitive {}: {} vertices, {} indices, {} triangles",
		primitive_index,
		vertices.len(),
		indices.len(),
		indices.len() / 3
	);

	Ok(ImportedPrimitive {
		mesh_index,
		primitive_index,
		base_color_image_index,
		normal_map,
		mesh: MeshData::new(vertices, indices),
		surface: material_surface(&material),
		material: MaterialData {
			base_color_image,
			base_color_factor,
			double_sided: material.double_sided(),
		},
	})
}

fn material_surface(material: &gltf::Material<'_>) -> renderer::vulkan::MaterialSurface {
	let alpha_cutoff = (material.alpha_mode() == gltf::material::AlphaMode::Mask)
		.then(|| material.alpha_cutoff().unwrap_or(0.5));
	let emissive_factor = if material.emissive_texture().is_none() {
		material.emissive_factor()
	} else {
		[0.0; 3]
	};
	renderer::vulkan::MaterialSurface {
		alpha_cutoff,
		emissive_factor,
	}
}

#[cfg(test)]
#[path = "../../../tests/unit/assets/gltf_loader/mesh.rs"]
mod tests;
