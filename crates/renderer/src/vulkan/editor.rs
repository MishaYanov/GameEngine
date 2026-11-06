//! Bounded editor passes recorded into the ordinary fenced frame.
use super::{CameraUniform, VulkanBuffer, VulkanDevice};
use crate::editor_visuals::{EditorVertex, EditorVisuals, MAX_EDITOR_LINES, append_line};
use ash::{Device, util::read_spv, vk};
use glam::{Mat4, Vec3};
use std::{io::Cursor, mem::size_of};

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

struct Pipeline {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,
	raw: vk::Pipeline,
	layout: vk::PipelineLayout,
}

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

impl Pipeline {
	fn new(
		device: &Device,
		color: vk::Format,
		depth: vk::Format,
		grid: bool,
		depth_test: bool,
	) -> Result<Self, String> {
		let (vertex, fragment) = if grid {
			(
				include_bytes!(concat!(env!("OUT_DIR"), "/editor-grid.vert.spv")).as_slice(),
				include_bytes!(concat!(env!("OUT_DIR"), "/editor-grid.frag.spv")).as_slice(),
			)
		} else {
			(
				include_bytes!(concat!(env!("OUT_DIR"), "/editor-lines.vert.spv")).as_slice(),
				include_bytes!(concat!(env!("OUT_DIR"), "/editor-lines.frag.spv")).as_slice(),
			)
		};
		let vertex = shader(device, vertex)?;
		let fragment = match shader(device, fragment) {
			Ok(shader) => shader,
			Err(error) => {
				unsafe {
					device.destroy_shader_module(vertex, None);
				}
				return Err(error);
			}
		};
		let ranges = if grid {
			vec![
				vk::PushConstantRange::default()
					.offset(0)
					.size(size_of::<GridPush>() as u32)
					.stage_flags(vk::ShaderStageFlags::FRAGMENT),
			]
		} else {
			vec![]
		};
		let layout = match unsafe {
			device.create_pipeline_layout(
				&vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&ranges),
				None,
			)
		} {
			Ok(layout) => layout,
			Err(error) => {
				unsafe {
					device.destroy_shader_module(vertex, None);
					device.destroy_shader_module(fragment, None);
				}
				return Err(error.to_string());
			}
		};
		let stages = [
			vk::PipelineShaderStageCreateInfo::default()
				.stage(vk::ShaderStageFlags::VERTEX)
				.module(vertex)
				.name(c"main"),
			vk::PipelineShaderStageCreateInfo::default()
				.stage(vk::ShaderStageFlags::FRAGMENT)
				.module(fragment)
				.name(c"main"),
		];
		let bindings = [vk::VertexInputBindingDescription {
			binding: 0,
			stride: size_of::<EditorVertex>() as u32,
			input_rate: vk::VertexInputRate::VERTEX,
		}];
		let attributes = [
			vk::VertexInputAttributeDescription {
				binding: 0,
				location: 0,
				format: vk::Format::R32G32B32A32_SFLOAT,
				offset: 0,
			},
			vk::VertexInputAttributeDescription {
				binding: 0,
				location: 1,
				format: vk::Format::R32G32B32A32_SFLOAT,
				offset: 16,
			},
		];
		let input = if grid {
			vk::PipelineVertexInputStateCreateInfo::default()
		} else {
			vk::PipelineVertexInputStateCreateInfo::default()
				.vertex_binding_descriptions(&bindings)
				.vertex_attribute_descriptions(&attributes)
		};
		let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
			.topology(vk::PrimitiveTopology::TRIANGLE_LIST);
		let viewport = vk::PipelineViewportStateCreateInfo::default()
			.viewport_count(1)
			.scissor_count(1);
		let raster = vk::PipelineRasterizationStateCreateInfo::default()
			.polygon_mode(vk::PolygonMode::FILL)
			.cull_mode(vk::CullModeFlags::NONE)
			.line_width(1.0);
		let multisample = vk::PipelineMultisampleStateCreateInfo::default()
			.rasterization_samples(vk::SampleCountFlags::TYPE_1);
		let depth_state = vk::PipelineDepthStencilStateCreateInfo::default()
			.depth_test_enable(depth_test)
			.depth_write_enable(false)
			.depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
		let attachments = [vk::PipelineColorBlendAttachmentState::default()
			.blend_enable(true)
			.src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
			.dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
			.color_blend_op(vk::BlendOp::ADD)
			.src_alpha_blend_factor(vk::BlendFactor::ONE)
			.dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
			.alpha_blend_op(vk::BlendOp::ADD)
			.color_write_mask(vk::ColorComponentFlags::RGBA)];
		let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
		let dynamics = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
		let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamics);
		let colors = [color];
		let mut rendering = vk::PipelineRenderingCreateInfo::default()
			.color_attachment_formats(&colors)
			.depth_attachment_format(depth);
		let info = vk::GraphicsPipelineCreateInfo::default()
			.push_next(&mut rendering)
			.stages(&stages)
			.vertex_input_state(&input)
			.input_assembly_state(&assembly)
			.viewport_state(&viewport)
			.rasterization_state(&raster)
			.multisample_state(&multisample)
			.depth_stencil_state(&depth_state)
			.color_blend_state(&blend)
			.dynamic_state(&dynamic)
			.layout(layout);
		let result =
			unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) };
		unsafe {
			device.destroy_shader_module(vertex, None);
			device.destroy_shader_module(fragment, None);
		}
		let raw = match result {
			Ok(pipelines) => pipelines[0],
			Err((pipelines, error)) => {
				unsafe {
					for pipeline in pipelines {
						device.destroy_pipeline(pipeline, None);
					}
					device.destroy_pipeline_layout(layout, None);
				}
				return Err(format!("Editor pipeline creation failed: {error}"));
			}
		};
		Ok(Self {
			_metrics: vec![
				crate::metrics::object(
					device,
					"pipeline",
					raw,
					"Editor visualization pipeline",
					if grid {
						"analytic grid"
					} else if depth_test {
						"depth-tested lines"
					} else {
						"overlay lines"
					},
				),
				crate::metrics::object(
					device,
					"pipeline_layout",
					layout,
					"Editor pipeline layout",
					"",
				),
			],
			device: device.clone(),
			raw,
			layout,
		})
	}
}

fn shader(device: &Device, bytes: &[u8]) -> Result<vk::ShaderModule, String> {
	let code = read_spv(&mut Cursor::new(bytes)).map_err(|e| e.to_string())?;
	unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None) }
		.map_err(|e| e.to_string())
}

impl Drop for Pipeline {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_pipeline(self.raw, None);
			self.device.destroy_pipeline_layout(self.layout, None);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vulkan::test_driver::{device, reset, state};
	use crate::{EditorGrid, EditorLine};
	fn pass(device: &VulkanDevice) -> EditorPass {
		let pipeline = || Pipeline {
			_metrics: Vec::new(),
			device: device.raw().clone(),
			raw: vk::Pipeline::null(),
			layout: vk::PipelineLayout::null(),
		};
		EditorPass {
			grid_pipeline: pipeline(),
			depth_lines: pipeline(),
			overlay_lines: pipeline(),
			color_format: vk::Format::B8G8R8A8_SRGB,
			depth_format: vk::Format::D32_SFLOAT,
			vertices: Vec::new(),
			vertex_buffer: None,
			capacity: 0,
			depth_count: 0,
			grid: None,
		}
	}
	#[test]
	fn repeated_editor_frames_reuse_mapped_storage_and_keep_depth_groups_separate() {
		reset();
		let device = device();
		let mut pass = pass(&device);
		let camera = CameraUniform::from_view_projection(Mat4::IDENTITY);
		let line = EditorLine::new([-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [1.0; 4]);
		let visuals = EditorVisuals {
			grid: Some(EditorGrid::default()),
			lines: vec![
				line,
				EditorLine {
					depth_test: true,
					..line
				},
			],
		};
		for _ in 0..3 {
			pass.prepare(&device, &visuals, &camera, [800, 600])
				.unwrap();
		}
		assert_eq!(pass.depth_count, 6);
		assert_eq!(pass.vertices.len(), 12);
		assert_eq!(pass.draw_counts(), (3, 5));
		state(|state| {
			assert_eq!(state.calls["create_buffer"], 1);
			assert_eq!(state.calls["allocate_memory"], 1);
			assert_eq!(state.calls["map_memory"], 1);
		});
		let capacity = pass.capacity;
		pass.prepare(&device, &EditorVisuals::default(), &camera, [800, 600])
			.unwrap();
		assert_eq!(pass.draw_counts(), (0, 0));
		assert_eq!(pass.capacity, capacity);
	}

	#[test]
	fn failed_editor_buffer_growth_can_be_retried_without_poisoning_capacity() {
		reset();
		let device = device();
		let mut pass = pass(&device);
		let camera = CameraUniform::from_view_projection(Mat4::IDENTITY);
		let visuals = EditorVisuals {
			grid: None,
			lines: vec![EditorLine::new([-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [1.0; 4])],
		};
		state(|state| state.fail = Some(("create_buffer", 1)));
		assert!(
			pass.prepare(&device, &visuals, &camera, [800, 600])
				.is_err()
		);
		assert_eq!(pass.capacity, 0);
		assert!(pass.vertex_buffer.is_none());
		state(|state| state.fail = None);
		pass.prepare(&device, &visuals, &camera, [800, 600])
			.unwrap();
		assert_eq!(pass.draw_counts(), (1, 2));
	}
}
