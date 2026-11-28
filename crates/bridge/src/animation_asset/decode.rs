use engine_core::animation::{AnimationAsset, MAX_ANIMATION_BYTES, build_envelopes, global_pose};
use sha2::{Digest, Sha256};

use super::{
	clips::decode_clips,
	identity::decode_character,
	mesh::decode_mesh,
	rig::{MAX_RIG_NODES, decode_rig},
};

const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_CLIPS: usize = 32;

pub fn decode(bytes: &[u8]) -> Result<AnimationAsset, String> {
	if bytes.len() > MAX_ANIMATION_BYTES {
		return Err("Animation GLB exceeds 32 MiB".into());
	}
	let container = gltf::binary::Glb::from_slice(bytes).map_err(|e| e.to_string())?;
	if container.json.len() > MAX_JSON_BYTES {
		return Err("Animation JSON chunk exceeds 2 MiB".into());
	}
	let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
	let doc = &gltf.document;
	let bin = gltf
		.blob
		.as_deref()
		.ok_or("Animation GLB has no BIN chunk")?;
	if doc.extensions_required().next().is_some()
		|| doc.nodes().len() > MAX_RIG_NODES
		|| doc.skins().len() != 1
		|| doc.animations().len() == 0
		|| doc.animations().len() > MAX_CLIPS
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
	let rig = decode_rig(scene, bin)?;
	let character = decode_character(doc, &rig)?;
	// Geometry, textures, and clips must all charge this same decoded byte budget.
	let mut remaining = MAX_ANIMATION_BYTES;
	let (primitives, images) = decode_mesh(rig.mesh, bin, rig.skin.joints.len(), &mut remaining)?;
	let clips = decode_clips(doc, bin, &rig.node_mapping, &mut remaining)?;
	let mut asset = AnimationAsset {
		source_sha256: format!("{:x}", Sha256::digest(bytes)),
		source_scene,
		character,
		nodes: rig.nodes,
		skin: rig.skin,
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

pub(super) fn charge(remaining: &mut usize, count: usize, stride: usize) -> Result<(), String> {
	*remaining = remaining
		.checked_sub(count.checked_mul(stride).ok_or("Animation size overflow")?)
		.ok_or("Animation decoded byte budget exceeded")?;
	Ok(())
}

pub(super) fn require_f32(
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
