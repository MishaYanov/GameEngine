use std::collections::BTreeMap;

use engine_core::animation::{
	AnimationImage, MAX_ANIMATION_BYTES, SkinPrimitive, SkinVertex, validated_influences,
};

use super::decode::{charge, require_f32};

const MAX_IMAGE_DIMENSION: u32 = 4096;

pub(super) fn decode_mesh(
	mesh: gltf::Mesh<'_>,
	bin: &[u8],
	joint_count: usize,
	remaining: &mut usize,
) -> Result<(Vec<SkinPrimitive>, Vec<AnimationImage>), String> {
	let mut vertices_total = 0usize;
	let mut indices_total = 0usize;
	let mut primitives = Vec::new();
	let mut image_map = BTreeMap::new();
	let mut images = Vec::new();
	for primitive in mesh.primitives() {
		if primitives.len() >= 256
			|| primitive.mode() != gltf::mesh::Mode::Triangles
			|| primitive.morph_targets().next().is_some()
			|| primitive.attributes().any(
				|(s, _)| matches!(s,gltf::Semantic::Joints(n)|gltf::Semantic::Weights(n)if n!=0),
			) {
			return Err("Unsupported animated primitive or influence set".into());
		}
		let position = primitive
			.get(&gltf::Semantic::Positions)
			.ok_or("Missing skin POSITION")?;
		require_f32(&position, gltf::accessor::Dimensions::Vec3)?;
		require_f32(
			&primitive
				.get(&gltf::Semantic::Normals)
				.ok_or("Missing skin NORMAL")?,
			gltf::accessor::Dimensions::Vec3,
		)?;
		if primitive.indices().is_some_and(|a| {
			a.dimensions() != gltf::accessor::Dimensions::Scalar
				|| a.normalized()
				|| !matches!(
					a.data_type(),
					gltf::accessor::DataType::U8
						| gltf::accessor::DataType::U16
						| gltf::accessor::DataType::U32
				)
		}) {
			return Err("Invalid skin index accessor".into());
		}
		if primitive
			.get(&gltf::Semantic::TexCoords(0))
			.is_some_and(|a| {
				a.dimensions() != gltf::accessor::Dimensions::Vec2
					|| !matches!(
						(a.data_type(), a.normalized()),
						(gltf::accessor::DataType::F32, false)
							| (
								gltf::accessor::DataType::U8 | gltf::accessor::DataType::U16,
								true
							)
					)
			}) {
			return Err("Invalid skin TEXCOORD_0 accessor".into());
		}

		let count = position.count();
		vertices_total = vertices_total
			.checked_add(count)
			.ok_or("Vertex count overflow")?;
		let n_indices = primitive.indices().map_or(count, |i| i.count());
		indices_total = indices_total
			.checked_add(n_indices)
			.ok_or("Index count overflow")?;
		if count == 0
			|| vertices_total > 100_000
			|| indices_total > 300_000
			|| n_indices == 0
			|| n_indices % 3 != 0
		{
			return Err("Animated geometry exceeds vertex/index bounds".into());
		}
		charge(remaining, count, std::mem::size_of::<SkinVertex>() + 48)?;
		charge(remaining, n_indices, 4)?;
		for semantic in [
			gltf::Semantic::Normals,
			gltf::Semantic::Joints(0),
			gltf::Semantic::Weights(0),
		] {
			if primitive.get(&semantic).is_none_or(|a| a.count() != count) {
				return Err("Skin attribute missing or count differs from POSITION".into());
			}
		}
		if primitive
			.get(&gltf::Semantic::TexCoords(0))
			.is_some_and(|a| a.count() != count)
		{
			return Err("TEXCOORD_0 count differs from POSITION".into());
		}
		let j = primitive.get(&gltf::Semantic::Joints(0)).unwrap();
		let w = primitive.get(&gltf::Semantic::Weights(0)).unwrap();
		if j.normalized()
			|| !matches!(
				j.data_type(),
				gltf::accessor::DataType::U8 | gltf::accessor::DataType::U16
			) || j.dimensions() != gltf::accessor::Dimensions::Vec4
			|| w.dimensions() != gltf::accessor::Dimensions::Vec4
			|| !matches!(
				(w.data_type(), w.normalized()),
				(gltf::accessor::DataType::F32, false)
					| (
						gltf::accessor::DataType::U8 | gltf::accessor::DataType::U16,
						true
					)
			) {
			return Err("Invalid joint/weight accessor encoding".into());
		}
		let reader = primitive.reader(|b| if b.index() == 0 { Some(bin) } else { None });
		let positions: Vec<_> = reader
			.read_positions()
			.ok_or("Missing positions")?
			.collect();
		let normals: Vec<_> = reader.read_normals().ok_or("Missing normals")?.collect();
		let joints_values: Vec<_> = reader
			.read_joints(0)
			.ok_or("Missing joints")?
			.into_u16()
			.collect();
		let weights: Vec<_> = reader
			.read_weights(0)
			.ok_or("Missing weights")?
			.into_f32()
			.collect();
		let uv: Vec<_> = reader
			.read_tex_coords(0)
			.map(|v| v.into_f32().collect())
			.unwrap_or_else(|| vec![[0.; 2]; count]);
		if [
			positions.len(),
			normals.len(),
			joints_values.len(),
			weights.len(),
			uv.len(),
		]
		.iter()
		.any(|n| *n != count)
		{
			return Err("Decoded attribute count mismatch".into());
		}
		let mut vertices = Vec::with_capacity(count);
		for i in 0..count {
			let normal = glam::Vec3::from_array(normals[i]);
			let normal = normal
				.try_normalize()
				.ok_or("Skin normal has zero or nonfinite length")?;
			if positions[i].iter().any(|v| !v.is_finite() || v.abs() > 1e6)
				|| uv[i].iter().any(|v| !v.is_finite())
				|| !normal.is_finite()
				|| normal.length_squared() < 1e-12
			{
				return Err("Nonfinite geometry or zero normal".into());
			}
			let (joints, weights) =
				validated_influences(joints_values[i], weights[i], joint_count)?;
			vertices.push(SkinVertex {
				position: positions[i],
				normal: normal.to_array(),
				uv: uv[i],
				joints,
				weights,
			});
		}
		let indices: Vec<u32> = reader
			.read_indices()
			.map(|v| v.into_u32().collect())
			.unwrap_or_else(|| (0..count as u32).collect());
		if indices.iter().any(|v| *v as usize >= count) {
			return Err("Animated index outside vertex buffer".into());
		}
		let material = primitive.material();
		if material.alpha_mode() != gltf::material::AlphaMode::Opaque {
			return Err("Animated material requires OPAQUE alpha mode".into());
		}
		let pbr = material.pbr_metallic_roughness();
		if pbr
			.base_color_factor()
			.iter()
			.any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
		{
			return Err("Animated base-color factors must be finite values in 0..=1".into());
		}
		if material.normal_texture().is_some()
			|| material.occlusion_texture().is_some()
			|| material.emissive_texture().is_some()
			|| pbr.metallic_roughness_texture().is_some()
		{
			return Err("Animated material supports base-color texture only".into());
		}
		let base_color_image = if let Some(info) = pbr.base_color_texture() {
			if info.tex_coord() != 0 || primitive.get(&gltf::Semantic::TexCoords(0)).is_none() {
				return Err("Animated base-color texture requires TEXCOORD_0".into());
			}
			let texture = info.texture();
			let sampler = texture.sampler();
			if sampler.wrap_s() != gltf::texture::WrappingMode::Repeat
				|| sampler.wrap_t() != gltf::texture::WrappingMode::Repeat
				|| sampler
					.mag_filter()
					.is_some_and(|v| v != gltf::texture::MagFilter::Linear)
				|| sampler
					.min_filter()
					.is_some_and(|v| v != gltf::texture::MinFilter::Linear)
			{
				return Err(
					"Animated texture supports linear/repeat sampling without mipmaps".into(),
				);
			}
			let source = texture.source();
			let original = source.index();
			if let Some(index) = image_map.get(&original) {
				Some(*index)
			} else {
				if images.len() >= 64 {
					return Err("Animation exceeds 64 material images".into());
				}
				let gltf::image::Source::View { view, mime_type } = source.source() else {
					return Err("Animation texture must be embedded".into());
				};
				let format = match mime_type {
					"image/png" => image::ImageFormat::Png,
					"image/jpeg" => image::ImageFormat::Jpeg,
					_ => return Err("Animation image must be PNG or JPEG".into()),
				};
				let end = view
					.offset()
					.checked_add(view.length())
					.ok_or("Image view overflow")?;
				let encoded = bin
					.get(view.offset()..end)
					.ok_or("Image view exceeds BIN")?;
				let (width, height) =
					image::ImageReader::with_format(std::io::Cursor::new(encoded), format)
						.into_dimensions()
						.map_err(|e| e.to_string())?;
				if width == 0
					|| height == 0 || width > MAX_IMAGE_DIMENSION
					|| height > MAX_IMAGE_DIMENSION
				{
					return Err("Animation image exceeds 4096-pixel dimension limit".into());
				}
				charge(remaining, width as usize * height as usize, 4)?;
				let mut reader =
					image::ImageReader::with_format(std::io::Cursor::new(encoded), format);
				let mut limits = image::Limits::default();
				limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
				limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
				limits.max_alloc = Some(MAX_ANIMATION_BYTES as u64);
				reader.limits(limits);
				let rgba = reader
					.decode()
					.map_err(|e| e.to_string())?
					.to_rgba8()
					.into_raw();
				let index = images.len();
				images.push(AnimationImage {
					width,
					height,
					rgba,
				});
				image_map.insert(original, index);
				Some(index)
			}
		} else {
			None
		};
		primitives.push(SkinPrimitive {
			vertices,
			indices,
			color: pbr.base_color_factor(),
			double_sided: material.double_sided(),
			material_index: material.index(),
			base_color_image,
		});
	}
	if primitives.is_empty() {
		return Err("Skin mesh has no primitives".into());
	}
	Ok((primitives, images))
}
