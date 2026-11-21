use std::{io::Cursor, mem::size_of};

use ash::{Device, util::read_spv, vk};

use crate::editor_visuals::EditorVertex;

use super::GridPush;

pub(super) struct Pipeline {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,
	pub(super) raw: vk::Pipeline,
	pub(super) layout: vk::PipelineLayout,
}

impl Pipeline {
	pub(super) fn new(
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

	#[cfg(test)]
	pub(super) fn for_test(device: &Device) -> Self {
		Self {
			_metrics: Vec::new(),
			device: device.clone(),
			raw: vk::Pipeline::null(),
			layout: vk::PipelineLayout::null(),
		}
	}
}

impl Drop for Pipeline {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_pipeline(self.raw, None);
			self.device.destroy_pipeline_layout(self.layout, None);
		}
	}
}

fn shader(device: &Device, bytes: &[u8]) -> Result<vk::ShaderModule, String> {
	let code = read_spv(&mut Cursor::new(bytes)).map_err(|e| e.to_string())?;
	unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None) }
		.map_err(|e| e.to_string())
}
