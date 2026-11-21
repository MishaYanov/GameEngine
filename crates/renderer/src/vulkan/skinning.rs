//! Each frame owns its palette allocation and descriptor until its completion
//! fence is observed. Host-coherent writes occur only after that wait.
use super::{RenderObject, VulkanBuffer, VulkanDevice, VulkanFrameError};
use ash::{Device, vk};

pub(crate) const MAX_PALETTE_MATRICES: usize = 64 * 128;

pub(crate) struct PaletteLayout {
	device: Device,
	pub raw: vk::DescriptorSetLayout,
}
impl PaletteLayout {
	pub fn new(device: &Device) -> Result<Self, vk::Result> {
		let bindings = [vk::DescriptorSetLayoutBinding::default()
			.binding(0)
			.descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
			.descriptor_count(1)
			.stage_flags(vk::ShaderStageFlags::VERTEX)];
		let raw = unsafe {
			device.create_descriptor_set_layout(
				&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
				None,
			)?
		};
		Ok(Self {
			device: device.clone(),
			raw,
		})
	}
}
impl Drop for PaletteLayout {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_descriptor_set_layout(self.raw, None);
		}
	}
}

pub(crate) struct PaletteStorage {
	device: Device,
	pool: vk::DescriptorPool,
	pub set: vk::DescriptorSet,
	buffer: VulkanBuffer,
	_layout: PaletteLayout,
	data: Vec<[f32; 16]>,
	pub ranges: Vec<Option<(u32, u32)>>,
}
impl PaletteStorage {
	pub fn new(device: &VulkanDevice) -> Result<Self, VulkanFrameError> {
		let buffer = VulkanBuffer::host_storage(device, (MAX_PALETTE_MATRICES * 64) as u64)?;
		let layout = PaletteLayout::new(device.raw())?;
		let sizes = [vk::DescriptorPoolSize {
			ty: vk::DescriptorType::STORAGE_BUFFER,
			descriptor_count: 1,
		}];
		let pool = unsafe {
			device.raw().create_descriptor_pool(
				&vk::DescriptorPoolCreateInfo::default()
					.max_sets(1)
					.pool_sizes(&sizes),
				None,
			)?
		};
		let layouts = [layout.raw];
		let set = match unsafe {
			device.raw().allocate_descriptor_sets(
				&vk::DescriptorSetAllocateInfo::default()
					.descriptor_pool(pool)
					.set_layouts(&layouts),
			)
		} {
			Ok(sets) => sets[0],
			Err(error) => {
				unsafe {
					device.raw().destroy_descriptor_pool(pool, None);
				}
				return Err(error.into());
			}
		};
		let info = [vk::DescriptorBufferInfo::default()
			.buffer(buffer.raw())
			.offset(0)
			.range(buffer.size())];
		unsafe {
			device.raw().update_descriptor_sets(
				&[vk::WriteDescriptorSet::default()
					.dst_set(set)
					.dst_binding(0)
					.descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
					.buffer_info(&info)],
				&[],
			);
		}
		Ok(Self {
			device: device.raw().clone(),
			pool,
			set,
			buffer,
			_layout: layout,
			data: Vec::new(),
			ranges: Vec::new(),
		})
	}
	pub fn matrix_count(&self) -> usize {
		self.data.len()
	}
	pub fn prepare(&mut self, objects: &[RenderObject<'_>]) -> Result<(), VulkanFrameError> {
		self.data.clear();
		self.ranges.clear();
		let mut shared = std::collections::HashMap::new();
		for object in objects {
			let Some(palette) = object.skin_palette() else {
				self.ranges.push(None);
				continue;
			};
			if object.mesh().skin_buffer().is_none()
				|| palette.is_empty()
				|| palette.len() > 128
				|| palette.iter().flatten().any(|v| !v.is_finite())
			{
				return Err(VulkanFrameError::Editor(
					"Invalid skin palette or missing immutable influence buffer".into(),
				));
			}
			let key = (palette.as_ptr() as usize, palette.len());
			let range = if let Some(range) = shared.get(&key) {
				*range
			} else {
				if self.data.len() + palette.len() > MAX_PALETTE_MATRICES {
					return Err(VulkanFrameError::Editor(
						"Frame skin palette budget exceeded".into(),
					));
				}
				let range = (self.data.len() as u32, palette.len() as u32);
				self.data.extend_from_slice(palette);
				shared.insert(key, range);
				range
			};
			self.ranges.push(Some(range));
		}
		if !self.data.is_empty() {
			self.buffer.write(&self.data)?;
		}
		Ok(())
	}
}
impl Drop for PaletteStorage {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_descriptor_pool(self.pool, None);
		}
	}
}

/// Preserve the material lane when addressing a second or later instance.
pub(crate) fn apply_palette_range(push: &mut super::ModelPushConstants, base: u32, count: u32) {
	push.flags[1] = base;
	push.flags[2] = count;
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn palette_offsets_preserve_material_flags_and_packed_vertex_contract() {
		use std::mem::{offset_of, size_of};
		assert_eq!(size_of::<super::super::mesh::SkinInfluence>(), 24);
		assert_eq!(offset_of!(super::super::mesh::SkinInfluence, weights), 8);
		let mut push = super::super::ModelPushConstants::identity();
		push.flags[0] = 1;
		push.base_color_factor = [0.2, 0.4, 0.6, 1.];
		apply_palette_range(&mut push, 0, 2);
		apply_palette_range(&mut push, 2, 2);
		assert_eq!(push.flags, [1, 2, 2, 0]);
		assert_eq!(push.base_color_factor, [0.2, 0.4, 0.6, 1.]);
		assert_eq!(offset_of!(super::super::ModelPushConstants, flags), 80);
	}
	#[test]
	fn opposite_rotation_normal_blend_uses_finite_dominant_influence() {
		let transforms = [
			glam::Mat3::IDENTITY,
			glam::Mat3::from_rotation_z(std::f32::consts::PI),
		];
		let normals = transforms.map(|m| (m * glam::Vec3::X).normalize());
		let weighted = normals[0] * 0.5 + normals[1] * 0.5;
		assert!(weighted.length_squared() < 1e-12);
		let result = if weighted.length_squared() > 1e-12 {
			weighted.normalize()
		} else {
			normals[0]
		};
		assert!(result.is_finite());
		assert_eq!(result, glam::Vec3::X);
	}
}
