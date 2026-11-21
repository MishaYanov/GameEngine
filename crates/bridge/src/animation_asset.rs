//! Shared bounded GLB animation decoding for editor, native player, compiler and headless.
use crate::runtime_prefab::{contained_file, read_bounded};
use engine_core::{
	SceneData,
	animation::*,
	asset_catalog::{AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES},
};
use glam::{Mat4, Quat};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
	collections::{BTreeMap, HashSet},
	path::Path,
	sync::Arc,
};

#[derive(Clone, Debug, Serialize)]
pub struct AnimationClipMetadata {
	pub index: u32,
	pub name: String,
	pub duration_seconds: f32,
}
#[derive(Clone, Debug, Serialize)]
pub struct AnimationMetadata {
	pub source: AnimationSourceRef,
	pub mesh_index: u32,
	pub nodes: usize,
	pub joints: usize,
	pub clips: Vec<AnimationClipMetadata>,
}
fn root_and_catalog(root: &Path) -> Result<(std::path::PathBuf, AssetCatalog), String> {
	let meta = std::fs::symlink_metadata(root).map_err(|e| e.to_string())?;
	if !meta.is_dir() || meta.file_type().is_symlink() {
		return Err("Animation root must be an ordinary directory".into());
	}
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let bytes = read_bounded(&contained_file(&root, CATALOG_FILE)?, MAX_CATALOG_BYTES)?;
	let catalog = AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
	Ok((root, catalog))
}
pub fn inspect(root: &Path, id: &AssetId) -> Result<AnimationMetadata, String> {
	let (root, catalog) = root_and_catalog(root)?;
	let entry = catalog
		.by_id(id)
		.ok_or("Animation asset ID is missing from catalog")?;
	if entry.kind != AssetKind::Model {
		return Err("Animation requires a model asset".into());
	}
	let asset = load_path(&root, &entry.path)?;
	Ok(AnimationMetadata {
		source: AnimationSourceRef {
			asset_id: id.clone(),
			expected_sha256: asset.source_sha256.clone(),
			scene_index: asset.source_scene,
			mesh_node: asset.skin.source_mesh_node,
			skin_index: asset.skin.source_skin,
		},
		mesh_index: asset.skin.mesh_index,
		nodes: asset.nodes.len(),
		joints: asset.skin.joints.len(),
		clips: asset
			.clips
			.iter()
			.map(|c| AnimationClipMetadata {
				index: c.source_index,
				name: c.name.clone(),
				duration_seconds: c.duration_seconds,
			})
			.collect(),
	})
}
pub fn load(root: &Path, source: &AnimationSourceRef) -> Result<Arc<AnimationAsset>, String> {
	let (root, catalog) = root_and_catalog(root)?;
	let entry = catalog
		.by_id(&source.asset_id)
		.ok_or("Animation asset ID is missing from catalog")?;
	if entry.kind != AssetKind::Model {
		return Err("Animation requires a model asset".into());
	}
	let asset = load_path(&root, &entry.path)?;
	if asset.source_sha256 != source.expected_sha256
		|| asset.source_scene != source.scene_index
		|| asset.skin.source_mesh_node != source.mesh_node
		|| asset.skin.source_skin != source.skin_index
	{
		return Err("Animation source changed; review and adopt its current version".into());
	}
	Ok(asset)
}
pub fn load_path(root: &Path, relative: &str) -> Result<Arc<AnimationAsset>, String> {
	if !relative.to_ascii_lowercase().ends_with(".glb") {
		return Err("Animation subset requires an embedded GLB".into());
	}
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let bytes = read_bounded(&contained_file(&root, relative)?, MAX_ANIMATION_BYTES)?;
	decode(&bytes).map(Arc::new)
}
pub fn prepare_scene(root: &Path, scene: &SceneData) -> Result<PreparedAnimationBindings, String> {
	prepare_scene_with_pins(root, scene, &BTreeMap::new())
}
/// Existing native models may pin a previously accepted version across unrelated
/// topology rebuilds. Identity/settings are still checked; only matching pins
/// bypass filesystem decoding. New worlds pass no pins and revalidate disk.
pub fn prepare_scene_with_pins(
	root: &Path,
	scene: &SceneData,
	pins: &PreparedAnimationBindings,
) -> Result<PreparedAnimationBindings, String> {
	let mut bindings = BTreeMap::new();
	let mut shared: BTreeMap<(AssetId, String), Arc<AnimationAsset>> = BTreeMap::new();
	if !scene.entities.iter().any(|e| e.animation.is_some()) {
		return Ok(bindings);
	}
	let (_, catalog) = root_and_catalog(root)?;
	let mut retained = 0usize;
	for entity in &scene.entities {
		let Some(component) = &entity.animation else {
			continue;
		};
		if bindings.len() >= MAX_ANIMATION_INSTANCES {
			return Err("Scene exceeds 64 animated instances".into());
		}
		let s = &component.settings;
		let entry = catalog
			.by_path(&entity.model_path)
			.ok_or("Animated mesh is not catalogued")?;
		if entry.id != s.source.asset_id
			|| entity
				.asset_id
				.as_ref()
				.is_some_and(|id| id != &s.source.asset_id)
		{
			return Err(
				"Mesh renderer and animation source have different asset identities".into(),
			);
		}
		let key = (s.source.asset_id.clone(), s.source.expected_sha256.clone());
		let asset = match shared.get(&key) {
			Some(asset) => Arc::clone(asset),
			None => {
				let asset = match pins
					.get(&entity.key)
					.filter(|asset| asset.validate_settings(s).is_ok())
				{
					Some(asset) => asset.clone(),
					None => load(root, &s.source)?,
				};
				retained = retained
					.checked_add(asset.retained_bytes())
					.ok_or("Animation world byte overflow")?;
				if retained > 128 * 1024 * 1024 {
					return Err("World animation CPU assets exceed 128 MiB".into());
				}
				shared.insert(key, Arc::clone(&asset));
				asset
			}
		};
		asset.validate_settings(s)?;
		if entity.mesh_index.unwrap_or(0) != asset.skin.mesh_index as usize {
			return Err("Animated mesh selection does not match skin mesh".into());
		}
		bindings.insert(entity.key.clone(), asset);
	}
	Ok(bindings)
}
fn charge(remaining: &mut usize, count: usize, stride: usize) -> Result<(), String> {
	*remaining = remaining
		.checked_sub(count.checked_mul(stride).ok_or("Animation size overflow")?)
		.ok_or("Animation decoded byte budget exceeded")?;
	Ok(())
}

// gltf's JSON validation is not a byte-layout validator. Its typed iterators
// assume nonzero counts and adequate strides, so establish those invariants
// before calling any Reader (including for unused accessors in the document).
fn validate_accessor_layouts(doc: &gltf::Document, bin: &[u8]) -> Result<(), String> {
	for view in doc.views() {
		let end = view
			.offset()
			.checked_add(view.length())
			.ok_or("GLB buffer view range overflow")?;
		if view.buffer().index() != 0 || end > view.buffer().length() || end > bin.len() {
			return Err("GLB buffer view exceeds its embedded buffer".into());
		}
	}
	for accessor in doc.accessors() {
		if accessor.count() == 0 || accessor.sparse().is_some() {
			return Err("Animation accessors must be nonempty and nonsparse".into());
		}
		let view = accessor
			.view()
			.ok_or("Animation accessor requires an explicit buffer view")?;
		let component = accessor.data_type().size();
		let element = match accessor.dimensions() {
			gltf::accessor::Dimensions::Mat2 => (2 * component).div_ceil(4) * 4 * 2,
			gltf::accessor::Dimensions::Mat3 => (3 * component).div_ceil(4) * 4 * 3,
			gltf::accessor::Dimensions::Mat4 => (4 * component).div_ceil(4) * 4 * 4,
			dimensions => component * dimensions.multiplicity(),
		};
		let stride = view.stride().unwrap_or(element);
		let start = view
			.offset()
			.checked_add(accessor.offset())
			.ok_or("Accessor offset overflow")?;
		let size = (accessor.count() - 1)
			.checked_mul(stride)
			.and_then(|n| n.checked_add(element))
			.ok_or("Accessor size overflow")?;
		let end = accessor
			.offset()
			.checked_add(size)
			.ok_or("Accessor range overflow")?;
		if stride < element
			|| stride % component != 0
			|| accessor.offset() % component != 0
			|| start % component != 0
			|| end > view.length()
		{
			return Err(format!(
				"Accessor {} has invalid stride, alignment, or range",
				accessor.index()
			));
		}
		if matches!(
			accessor.dimensions(),
			gltf::accessor::Dimensions::Mat2
				| gltf::accessor::Dimensions::Mat3
				| gltf::accessor::Dimensions::Mat4
		) && start % 4 != 0
		{
			return Err("Matrix accessor columns must be four-byte aligned".into());
		}
	}
	Ok(())
}

fn require_f32(
	accessor: &gltf::Accessor<'_>,
	dimensions: gltf::accessor::Dimensions,
) -> Result<(), String> {
	if accessor.data_type() != gltf::accessor::DataType::F32
		|| accessor.dimensions() != dimensions
		|| accessor.normalized()
	{
		return Err(format!(
			"Accessor {} has unsupported semantic encoding",
			accessor.index()
		));
	}
	Ok(())
}
pub fn decode(bytes: &[u8]) -> Result<AnimationAsset, String> {
	if bytes.len() > MAX_ANIMATION_BYTES {
		return Err("Animation GLB exceeds 32 MiB".into());
	}
	let container = gltf::binary::Glb::from_slice(bytes).map_err(|e| e.to_string())?;
	if container.json.len() > 2 * 1024 * 1024 {
		return Err("Animation JSON chunk exceeds 2 MiB".into());
	}
	let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
	let doc = &gltf.document;
	let bin = gltf
		.blob
		.as_deref()
		.ok_or("Animation GLB has no BIN chunk")?;
	if doc.extensions_required().next().is_some()
		|| doc.nodes().len() > 256
		|| doc.skins().len() != 1
		|| doc.animations().len() == 0
		|| doc.animations().len() > 32
	{
		return Err("Animation subset requires bounded nodes, 1 skin, 1..32 clips and no required extensions".into());
	}
	if doc.buffers().len() != 1
		|| doc
			.buffers()
			.any(|b| !matches!(b.source(), gltf::buffer::Source::Bin) || b.length() > bin.len())
		|| doc
			.images()
			.any(|i| !matches!(i.source(), gltf::image::Source::View { .. }))
	{
		return Err("Animation GLB must embed all buffers and images".into());
	}
	if doc.accessors().any(|a| a.sparse().is_some()) {
		return Err("Sparse accessors are outside the animation subset".into());
	}
	validate_accessor_layouts(doc, bin)?;
	let scene = doc
		.default_scene()
		.or_else(|| doc.scenes().next())
		.ok_or("Animation GLB has no scene")?;
	let source_scene = scene.index() as u32;
	let mut mapping = BTreeMap::new();
	let mut nodes = Vec::new();
	let mut pending: Vec<_> = scene
		.nodes()
		.collect::<Vec<_>>()
		.into_iter()
		.rev()
		.map(|n| (n, None, 0usize))
		.collect();
	let mut mesh = None;
	while let Some((node, parent, depth)) = pending.pop() {
		if depth > 128 || mapping.contains_key(&node.index()) || nodes.len() >= 256 {
			return Err("Animation hierarchy exceeds bounds or repeats a node".into());
		}
		if node.weights().is_some() {
			return Err("Morph animation is unsupported".into());
		}
		let (t, r, s) = node.transform().decomposed();
		let rest = LocalTrs {
			translation: t,
			rotation_xyzw: r,
			scale: s,
		};
		rest.validate()?;
		let original = Mat4::from_cols_array_2d(&node.transform().matrix());
		checked_similarity_inverse(original)?;
		if original
			.to_cols_array()
			.iter()
			.zip(rest.matrix().to_cols_array())
			.any(|(a, b)| (*a - b).abs() > 1e-4 * (1. + a.abs()))
		{
			return Err("Animated matrix cannot be represented as TRS".into());
		}
		let dense = nodes.len() as u16;
		mapping.insert(node.index(), dense);
		nodes.push(RigNode {
			source_node: node.index() as u32,
			parent,
			rest,
		});
		if node.mesh().is_some() {
			if mesh.is_some() || node.skin().is_none() {
				return Err("Animation subset requires exactly one skinned mesh node".into());
			}
			mesh = Some((dense, node.clone()));
		}
		for child in node.children().collect::<Vec<_>>().into_iter().rev() {
			pending.push((child, Some(dense), depth + 1));
		}
	}
	let (mesh_node, node) = mesh.ok_or("Animation scene has no skinned mesh")?;
	let skin = node.skin().ok_or("Mesh has no skin")?;
	let source_skin = skin.index() as u32;
	let source_mesh_node = node.index() as u32;
	let joints: Vec<u16> = skin
		.joints()
		.map(|n| {
			mapping
				.get(&n.index())
				.copied()
				.ok_or("Joint lies outside selected scene")
		})
		.collect::<Result<_, _>>()?;
	if joints.is_empty()
		|| joints.len() > 128
		|| joints.iter().collect::<HashSet<_>>().len() != joints.len()
	{
		return Err("Skin requires 1..128 distinct joints".into());
	}
	let ancestors = |mut n: u16| {
		let mut out = Vec::new();
		loop {
			out.push(n);
			match nodes[usize::from(n)].parent {
				Some(p) => n = p,
				None => break,
			}
		}
		out
	};
	let common = ancestors(joints[0]);
	if !common
		.iter()
		.any(|n| joints.iter().all(|j| ancestors(*j).contains(n)))
	{
		return Err("Skin joints have no common scene ancestor".into());
	}
	if let Some(root) = skin.skeleton() {
		let root = mapping
			.get(&root.index())
			.ok_or("Skeleton root outside scene")?;
		if joints.iter().any(|j| !ancestors(*j).contains(root)) {
			return Err("Skeleton root does not own all joints".into());
		}
	}
	if skin
		.inverse_bind_matrices()
		.is_some_and(|a| a.count() != joints.len())
	{
		return Err("Inverse bind count differs from joint count".into());
	}
	if let Some(accessor) = skin.inverse_bind_matrices() {
		require_f32(&accessor, gltf::accessor::Dimensions::Mat4)?;
	}
	let reader = skin.reader(|b| if b.index() == 0 { Some(bin) } else { None });
	let inverse_bind: Vec<Mat4> = match reader.read_inverse_bind_matrices() {
		Some(v) => v.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
		None => vec![Mat4::IDENTITY; joints.len()],
	};
	if inverse_bind.len() != joints.len() {
		return Err("Inverse bind count differs from skin joint count".into());
	}
	for m in &inverse_bind {
		checked_similarity_inverse(*m)?;
		if m.w_axis.truncate().abs().max_element() > 1e6 {
			return Err("Inverse-bind translation exceeds animation coordinate bounds".into());
		}
	}
	let mesh = node.mesh().ok_or("Missing skin mesh")?;
	let mesh_index = mesh.index() as u32;
	let mut remaining = MAX_ANIMATION_BYTES;
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
		charge(
			&mut remaining,
			count,
			std::mem::size_of::<SkinVertex>() + 48,
		)?;
		charge(&mut remaining, n_indices, 4)?;
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
				validated_influences(joints_values[i], weights[i], joints.len())?;
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
				if width == 0 || height == 0 || width > 4096 || height > 4096 {
					return Err("Animation image exceeds 4096-pixel dimension limit".into());
				}
				charge(&mut remaining, width as usize * height as usize, 4)?;
				let mut reader =
					image::ImageReader::with_format(std::io::Cursor::new(encoded), format);
				let mut limits = image::Limits::default();
				limits.max_image_width = Some(4096);
				limits.max_image_height = Some(4096);
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
	let mut clips = Vec::new();
	let mut total_keys = 0usize;
	for animation in doc.animations() {
		let mut tracks = Vec::new();
		let mut seen = HashSet::new();
		let mut duration = 0_f32;
		for channel in animation.channels() {
			if tracks.len() >= 768 {
				return Err("Animation clip exceeds 768 channels".into());
			}
			let target = channel.target();
			let dense = *mapping
				.get(&target.node().index())
				.ok_or("Animation target outside scene")?;
			let property = target.property();
			if !seen.insert((dense, format!("{property:?}"))) {
				return Err("Duplicate animation node/property channel".into());
			}
			let sampler = channel.sampler();
			require_f32(&sampler.input(), gltf::accessor::Dimensions::Scalar)?;
			match property {
				gltf::animation::Property::Translation | gltf::animation::Property::Scale => {
					require_f32(&sampler.output(), gltf::accessor::Dimensions::Vec3)?
				}
				gltf::animation::Property::Rotation => {
					let a = sampler.output();
					if a.dimensions() != gltf::accessor::Dimensions::Vec4
						|| !matches!(
							(a.data_type(), a.normalized()),
							(gltf::accessor::DataType::F32, false)
								| (
									gltf::accessor::DataType::I8
										| gltf::accessor::DataType::U8 | gltf::accessor::DataType::I16
										| gltf::accessor::DataType::U16,
									true
								)
						) {
						return Err("Invalid rotation output encoding".into());
					}
				}
				_ => return Err("Morph animation is outside the subset".into()),
			}
			let interpolation = match sampler.interpolation() {
				gltf::animation::Interpolation::Step => Interpolation::Step,
				gltf::animation::Interpolation::Linear => Interpolation::Linear,
				_ => return Err("CUBICSPLINE is outside the animation subset".into()),
			};
			let count = sampler.input().count();
			total_keys = total_keys.checked_add(count).ok_or("Key count overflow")?;
			if count > 4096 || total_keys > 262144 || sampler.output().count() != count {
				return Err("Animation key count exceeds bounds or differs from output".into());
			}
			charge(&mut remaining, count, 20)?;
			let reader = channel.reader(|b| if b.index() == 0 { Some(bin) } else { None });
			let times: Vec<_> = reader
				.read_inputs()
				.ok_or("Missing animation times")?
				.collect();
			use gltf::animation::util::ReadOutputs;
			let values = match reader.read_outputs().ok_or("Missing animation values")? {
				ReadOutputs::Translations(v) => {
					let v: Vec<_> = v.collect();
					if v.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e6) {
						return Err("Invalid translation keys".into());
					}
					TrackValues::Translation(v)
				}
				ReadOutputs::Scales(v) => {
					let v: Vec<_> = v.collect();
					for scale in &v {
						LocalTrs {
							translation: [0.; 3],
							rotation_xyzw: [0., 0., 0., 1.],
							scale: *scale,
						}
						.validate()?;
					}
					TrackValues::Scale(v)
				}
				ReadOutputs::Rotations(v) => {
					let mut v: Vec<_> = v.into_f32().collect();
					for value in &mut v {
						let q = Quat::from_array(*value);
						if !q.is_finite() || (q.length_squared() - 1.).abs() > 1e-3 {
							return Err("Invalid rotation keys".into());
						}
						*value = q.normalize().to_array();
					}
					TrackValues::Rotation(v)
				}
				_ => return Err("Morph animation is outside the subset".into()),
			};
			validate_times(&times, values.len())?;
			duration = duration.max(*times.last().unwrap());
			tracks.push(Track {
				node: dense,
				interpolation,
				times,
				values,
			});
		}
		if tracks.is_empty() {
			return Err("Animation clip has no supported channels".into());
		}
		let name = animation
			.name()
			.map(str::to_owned)
			.unwrap_or_else(|| format!("Clip {}", animation.index()));
		if name.len() > 1024 {
			return Err("Animation clip name exceeds 1024 bytes".into());
		}
		clips.push(Clip {
			source_index: animation.index() as u32,
			name,
			duration_seconds: duration,
			tracks,
		});
	}
	let mut asset = AnimationAsset {
		source_sha256: format!("{:x}", Sha256::digest(bytes)),
		source_scene,
		nodes,
		skin: Skin {
			mesh_node,
			source_mesh_node,
			source_skin,
			mesh_index,
			joints,
			inverse_bind,
		},
		clips,
		primitives,
		images,
		envelopes: Vec::new(),
	};
	asset.envelopes = build_envelopes(&asset)?;
	global_pose(
		&asset,
		&asset.nodes.iter().map(|n| n.rest).collect::<Vec<_>>(),
	)?;
	if asset.retained_bytes() > MAX_ANIMATION_BYTES {
		return Err("Retained animation exceeds 32 MiB".into());
	}
	Ok(asset)
}
