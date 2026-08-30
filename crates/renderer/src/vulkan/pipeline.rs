use super::{ModelPushConstants, Vertex};
use ash::{Device, util::read_spv, vk};
use std::io::Cursor;

pub struct VulkanGraphicsPipeline {
    device: Device,
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
}

impl VulkanGraphicsPipeline {
    pub fn new(
        device: &Device,
        color_format: vk::Format,
        depth_format: vk::Format,

        camera_layout: vk::DescriptorSetLayout,

        texture_layout: vk::DescriptorSetLayout,
    ) -> Result<Self, String> {
        let vertex_shader = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/triangle.vert.spv")),
        )?;

        let fragment_shader = match create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/triangle.frag.spv")),
        ) {
            Ok(shader) => shader,

            Err(error) => {
                unsafe {
                    device.destroy_shader_module(vertex_shader, None);
                }

                return Err(error);
            }
        };

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(ModelPushConstants::OFFSET)
            .size(ModelPushConstants::SIZE)];

        let descriptor_set_layouts = [camera_layout, texture_layout];

        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&descriptor_set_layouts)
            .push_constant_ranges(&push_constant_ranges);

        let layout = match unsafe { device.create_pipeline_layout(&layout_info, None) } {
            Ok(layout) => layout,

            Err(error) => {
                unsafe {
                    device.destroy_shader_module(vertex_shader, None);

                    device.destroy_shader_module(fragment_shader, None);
                }

                return Err(format!("failed to create pipeline layout: {error:?}"));
            }
        };

        let entry_point = c"main";

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex_shader)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment_shader)
                .name(entry_point),
        ];

        let vertex_binding_descriptions = [Vertex::binding_description()];

        let vertex_attribute_descriptions = Vertex::attribute_descriptions();

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_binding_descriptions)
            .vertex_attribute_descriptions(&vertex_attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::BACK)
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState {
            blend_enable: vk::FALSE,

            color_write_mask: vk::ColorComponentFlags::RGBA,

            ..Default::default()
        }];

        let color_blending =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachment);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];

        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let color_formats = [color_format];

        let mut rendering_info = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_formats)
            .depth_attachment_format(depth_format);

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .push_next(&mut rendering_info)
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state)
            .layout(layout)
            .render_pass(vk::RenderPass::null());

        let result = unsafe {
            device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
        };

        unsafe {
            device.destroy_shader_module(vertex_shader, None);

            device.destroy_shader_module(fragment_shader, None);
        }

        let pipelines = match result {
            Ok(pipelines) => pipelines,

            Err((_, error)) => {
                unsafe {
                    device.destroy_pipeline_layout(layout, None);
                }

                return Err(format!("failed to create graphics pipeline: {error:?}"));
            }
        };

        Ok(Self {
            device: device.clone(),
            pipeline: pipelines[0],
            layout,
        })
    }

    pub fn raw(&self) -> vk::Pipeline {
        self.pipeline
    }

    pub fn layout(&self) -> vk::PipelineLayout {
        self.layout
    }
}

impl Drop for VulkanGraphicsPipeline {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);

            self.device.destroy_pipeline_layout(self.layout, None);
        }
    }
}

fn create_shader_module(device: &Device, bytes: &[u8]) -> Result<vk::ShaderModule, String> {
    let mut cursor = Cursor::new(bytes);

    let code =
        read_spv(&mut cursor).map_err(|error| format!("failed to read SPIR-V shader: {error}"))?;

    let create_info = vk::ShaderModuleCreateInfo::default().code(&code);

    unsafe { device.create_shader_module(&create_info, None) }
        .map_err(|error| format!("failed to create Vulkan shader module: {error:?}"))
}
