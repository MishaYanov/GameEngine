use ash::vk::Handle;

use crate::vulkan::{RenderObject, instancing::InstanceData};

#[derive(Clone, Copy)]
pub(super) struct DrawBatch {
	pub(super) object: usize,
	pub(super) first_instance: u32,
	pub(super) count: u32,
	pub(super) instanced: bool,
}

pub(super) fn build(
	objects: &[RenderObject<'_>],
	order: &mut Vec<usize>,
	data: &mut Vec<InstanceData>,
	batches: &mut Vec<DrawBatch>,
) {
	order.clear();
	order.extend(0..objects.len());
	// Reorder only opaque runs. Every overlay/unlit/alpha object is an
	// ordering barrier, so editor UI and debug draws retain submission order.
	let mut start = 0;
	while start < objects.len() {
		if !batchable(&objects[start]) {
			start += 1;
			continue;
		}
		let end = (start..objects.len())
			.find(|&i| !batchable(&objects[i]))
			.unwrap_or(objects.len());
		order[start..end].sort_unstable_by_key(|&i| (batch_key(&objects[i]), i));
		start = end;
	}
	data.clear();
	batches.clear();
	for &index in order.iter() {
		let object = &objects[index];
		let instanced = batchable(object);
		let first_instance = data.len() as u32;
		data.push(InstanceData::from_object(object));
		if let Some(batch) = batches.last_mut()
			&& instanced
			&& batch.instanced
			&& batch_key(&objects[batch.object]) == batch_key(object)
		{
			batch.count += 1;
		} else {
			batches.push(DrawBatch {
				object: index,
				first_instance,
				count: 1,
				instanced,
			});
		}
	}
}

fn batchable(object: &RenderObject<'_>) -> bool {
	object.uses_full_mesh()
		&& object.skin_palette().is_none()
		&& object.transform().flags[0] == 0
		&& object.transform().base_color_factor[3] >= 1.0
		&& object.material().base_color_factor()[3] >= 1.0
}

fn batch_key(object: &RenderObject<'_>) -> [u64; 15] {
	let color = object.material().base_color_factor();
	let surface = object.material().surface();
	let normal_map = object.material().normal_map();
	[
		object.mesh().vertex_buffer().raw().as_raw(),
		object.mesh().index_buffer().raw().as_raw(),
		object.material().descriptor_set().as_raw(),
		object.material().double_sided() as u64,
		color[0].to_bits() as u64,
		color[1].to_bits() as u64,
		color[2].to_bits() as u64,
		color[3].to_bits() as u64,
		surface
			.alpha_cutoff
			.map_or(u64::MAX, |cutoff| u64::from(cutoff.to_bits())),
		surface.emissive_factor[0].to_bits() as u64,
		surface.emissive_factor[1].to_bits() as u64,
		surface.emissive_factor[2].to_bits() as u64,
		u64::from(object.visible())
			| u64::from(object.shadow_caster()) << 1
			| u64::from(object.reflected()) << 2,
		normal_map.map_or(0, |(set, _)| set.as_raw()),
		normal_map.map_or(0, |(_, scale)| u64::from(scale.to_bits())),
	]
}
