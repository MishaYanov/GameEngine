use std::collections::{BTreeMap, HashSet};

use engine_core::animation::{LocalTrs, RigNode, Skin, checked_similarity_inverse};
use glam::Mat4;

use super::decode::require_f32;

pub(super) const MAX_RIG_NODES: usize = 256;

pub(super) struct DecodedRig<'doc> {
	pub(super) nodes: Vec<RigNode>,
	pub(super) node_mapping: BTreeMap<usize, u16>,
	pub(super) skin: Skin,
	pub(super) mesh: gltf::Mesh<'doc>,
}

pub(super) fn decode_rig<'doc>(
	scene: gltf::Scene<'doc>,
	bin: &[u8],
) -> Result<DecodedRig<'doc>, String> {
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
		if depth > 128 || mapping.contains_key(&node.index()) || nodes.len() >= MAX_RIG_NODES {
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
			name: node
				.name()
				.map(|name| {
					name.chars()
						.filter(|ch| !ch.is_control())
						.take(256)
						.collect::<String>()
				})
				.filter(|name| !name.is_empty()),
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
	Ok(DecodedRig {
		nodes,
		node_mapping: mapping,
		skin: Skin {
			mesh_node,
			source_mesh_node,
			source_skin,
			mesh_index,
			joints,
			inverse_bind,
		},
		mesh,
	})
}
