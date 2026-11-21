//! Bounded editor passes recorded into the ordinary fenced frame.
use super::{CameraUniform, VulkanBuffer, VulkanDevice};
use crate::editor_visuals::{EditorVertex, EditorVisuals, MAX_EDITOR_LINES, append_line};
use ash::{Device, vk};
use glam::{Mat4, Vec3};
use std::mem::size_of;

mod pipeline;

use pipeline::Pipeline;

#[repr(C)]
#[derive(Clone, Copy)]
struct GridPush {
	inverse: [f32; 16],
	origin_spacing: [f32; 4],
	tangent_opacity: [f32; 4],
	normal_fade: [f32; 4],
	viewport: [f32; 4],
}

const _: () = assert!(size_of::<GridPush>() == 128);

pub(crate) struct EditorPass {
	grid_pipeline: Pipeline,
	depth_lines: Pipeline,
	overlay_lines: Pipeline,
	color_format: vk::Format,
	depth_format: vk::Format,
	vertices: Vec<EditorVertex>,
	vertex_buffer: Option<VulkanBuffer>,
	capacity: usize,
	depth_count: u32,
	grid: Option<GridPush>,
}

impl EditorPass {
	pub(crate) fn new(
		device: &VulkanDevice,
		color_format: vk::Format,
		depth_format: vk::Format,
	) -> Result<Self, String> {
		Ok(Self {
			grid_pipeline: Pipeline::new(device.raw(), color_format, depth_format, true, true)?,
			depth_lines: Pipeline::new(device.raw(), color_format, depth_format, false, true)?,
			overlay_lines: Pipeline::new(device.raw(), color_format, depth_format, false, false)?,
			color_format,
			depth_format,
			vertices: Vec::new(),
			vertex_buffer: None,
			capacity: 0,
			depth_count: 0,
			grid: None,
		})
	}

	pub(crate) fn supports_formats(&self, color: vk::Format, depth: vk::Format) -> bool {
		self.color_format == color && self.depth_format == depth
	}

	/// Caller must have completed the previous frame before replacing/writing buffers.
	pub(crate) fn prepare(
		&mut self,
		device: &VulkanDevice,
		visuals: &EditorVisuals,
		camera: &CameraUniform,
		extent: [u32; 2],
	) -> Result<(), String> {
		let matrix = Mat4::from_cols_array(&camera.view_projection);
		if !matrix.is_finite() {
			return Err("Editor camera matrix must be finite".into());
		}
		self.grid = if let Some(grid) = visuals.grid.filter(|grid| grid.opacity > 0.0) {
			let inverse = matrix.inverse();
			if !inverse.is_finite() {
				return Err("Editor grid requires an invertible camera matrix".into());
			}
			let normal = Vec3::from_array(grid.normal).normalize();
			let tangent = Vec3::from_array(grid.tangent);
			let tangent = (tangent - normal * normal.dot(tangent)).normalize();
			Some(GridPush {
				inverse: inverse.to_cols_array(),
				origin_spacing: [grid.origin[0], grid.origin[1], grid.origin[2], grid.spacing],
				tangent_opacity: [tangent.x, tangent.y, tangent.z, grid.opacity],
				normal_fade: [normal.x, normal.y, normal.z, grid.fade_distance],
				viewport: [extent[0] as f32, extent[1] as f32, 0.0, 0.0],
			})
		} else {
			None
		};
		self.vertices.clear();
		for line in visuals.lines.iter().filter(|line| line.depth_test) {
			append_line(&mut self.vertices, line, matrix, extent);
		}
		self.depth_count = self.vertices.len() as u32;
		for line in visuals.lines.iter().filter(|line| !line.depth_test) {
			append_line(&mut self.vertices, line, matrix, extent);
		}
		if self.vertices.len() > self.capacity {
			let capacity = self
				.vertices
				.len()
				.next_power_of_two()
				.min(MAX_EDITOR_LINES * 6);
			let buffer =
				VulkanBuffer::host_vertex(device, (capacity * size_of::<EditorVertex>()) as u64)
					.map_err(|e| e.to_string())?;
			self.vertex_buffer = Some(buffer);
			self.capacity = capacity;
		}
		if !self.vertices.is_empty() {
			self.vertex_buffer
				.as_ref()
				.expect("allocated editor buffer")
				.write(&self.vertices)
				.map_err(|e| e.to_string())?;
		}
		Ok(())
	}

	pub(crate) fn record(&self, device: &Device, command: vk::CommandBuffer) {
		unsafe {
			if let Some(grid) = self.grid {
				device.cmd_bind_pipeline(
					command,
					vk::PipelineBindPoint::GRAPHICS,
					self.grid_pipeline.raw,
				);
				let bytes = std::slice::from_raw_parts(
					(&grid as *const GridPush).cast::<u8>(),
					size_of::<GridPush>(),
				);
				device.cmd_push_constants(
					command,
					self.grid_pipeline.layout,
					vk::ShaderStageFlags::FRAGMENT,
					0,
					bytes,
				);
				device.cmd_draw(command, 3, 1, 0, 0);
			}
			if self.vertices.is_empty() {
				return;
			}
			device.cmd_bind_vertex_buffers(
				command,
				0,
				&[self
					.vertex_buffer
					.as_ref()
					.expect("prepared editor vertices")
					.raw()],
				&[0],
			);
			if self.depth_count > 0 {
				device.cmd_bind_pipeline(
					command,
					vk::PipelineBindPoint::GRAPHICS,
					self.depth_lines.raw,
				);
				device.cmd_draw(command, self.depth_count, 1, 0, 0);
			}
			let overlay_count = self.vertices.len() as u32 - self.depth_count;
			if overlay_count > 0 {
				device.cmd_bind_pipeline(
					command,
					vk::PipelineBindPoint::GRAPHICS,
					self.overlay_lines.raw,
				);
				device.cmd_draw(command, overlay_count, 1, self.depth_count, 0);
			}
		}
	}

	pub(crate) fn draw_counts(&self) -> (u64, u64) {
		let grid = u64::from(self.grid.is_some());
		let lines = u64::from(self.depth_count > 0)
			+ u64::from(self.vertices.len() > self.depth_count as usize);
		(grid + lines, grid + self.vertices.len() as u64 / 3)
	}
}

#[cfg(test)]
#[path = "../../tests/unit/vulkan/editor.rs"]
mod tests;
