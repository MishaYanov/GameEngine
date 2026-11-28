use super::{ModelPushConstants, Vertex, instancing::InstanceData};
use ash::{Device, util::read_spv, vk};
use std::io::Cursor;

struct PipelineOptions {
	double_sided: bool,
	overlay: bool,
	skinned: bool,
	pass: MeshPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshPass {
	Surface,
	Shadow,
	WireSurface,
	WireEdges,
	VertexMarkers,
}

impl MeshPass {
	fn depth_read_only(self) -> bool {
		matches!(self, Self::WireEdges | Self::VertexMarkers)
	}
}

struct EditorMeshPipelines {
	ordinary: VulkanGraphicsPipeline,
	skinned: VulkanGraphicsPipeline,
	double_sided: Option<(VulkanGraphicsPipeline, VulkanGraphicsPipeline)>,
}

impl EditorMeshPipelines {
	fn select(&self, skinned: bool, double_sided: bool) -> &VulkanGraphicsPipeline {
		if let Some((ordinary, skin)) = self.double_sided.as_ref().filter(|_| double_sided) {
			if skinned { skin } else { ordinary }
		} else if skinned {
			&self.skinned
		} else {
			&self.ordinary
		}
	}
}

pub struct VulkanMaterialPipelines {
	color_format: vk::Format,
	depth_format: vk::Format,
	single_sided: VulkanGraphicsPipeline,
	double_sided: VulkanGraphicsPipeline,
	overlay: VulkanGraphicsPipeline,
	skinned_single: VulkanGraphicsPipeline,
	skinned_double: VulkanGraphicsPipeline,
	wire_surface: Option<EditorMeshPipelines>,
	wire_edges: Option<EditorMeshPipelines>,
	vertex_markers: Option<EditorMeshPipelines>,
	shadows: Option<EditorMeshPipelines>,
}

impl VulkanMaterialPipelines {
	#[cfg(test)]
	pub(super) fn for_test(device: &Device) -> Self {
		let pipeline = || VulkanGraphicsPipeline {
			_metrics: Vec::new(),
			device: device.clone(),
			pipeline: vk::Pipeline::null(),
			layout: vk::PipelineLayout::null(),
			_palette_layout: None,
		};
		Self {
			color_format: vk::Format::B8G8R8A8_SRGB,
			depth_format: vk::Format::D32_SFLOAT,
			single_sided: pipeline(),
			double_sided: pipeline(),
			overlay: pipeline(),
			skinned_single: pipeline(),
			skinned_double: pipeline(),
			wire_surface: None,
			wire_edges: None,
			vertex_markers: None,
			shadows: None,
		}
	}
	pub fn new(
		device: &Device,
		color_format: vk::Format,
		depth_format: vk::Format,
		camera_layout: vk::DescriptorSetLayout,
		texture_layout: vk::DescriptorSetLayout,
	) -> Result<Self, String> {
		let single_sided = VulkanGraphicsPipeline::new(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			false,
			false,
		)?;
		let double_sided = VulkanGraphicsPipeline::new(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			true,
			false,
		)?;
		let overlay = VulkanGraphicsPipeline::new(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			true,
			true,
		)?;
		let skinned_single = VulkanGraphicsPipeline::new_kind(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			PipelineOptions {
				double_sided: false,
				overlay: false,
				skinned: true,
				pass: MeshPass::Surface,
			},
		)?;
		let skinned_double = VulkanGraphicsPipeline::new_kind(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			PipelineOptions {
				double_sided: true,
				overlay: false,
				skinned: true,
				pass: MeshPass::Surface,
			},
		)?;
		let shadow = |skinned, double_sided| {
			VulkanGraphicsPipeline::new_kind(
				device,
				vk::Format::UNDEFINED,
				super::shadow::ShadowMap::FORMAT,
				camera_layout,
				texture_layout,
				PipelineOptions {
					double_sided,
					overlay: false,
					skinned,
					pass: MeshPass::Shadow,
				},
			)
		};
		// Front-face culling matches the color pass. Double-sided foliage
		// retains its cutout silhouettes with either face turned to the light.
		let shadows = EditorMeshPipelines {
			ordinary: shadow(false, false)?,
			skinned: shadow(true, false)?,
			double_sided: Some((shadow(false, true)?, shadow(true, true)?)),
		};
		Ok(Self {
			skinned_single,
			skinned_double,
			color_format,
			depth_format,
			single_sided,
			double_sided,
			overlay,
			wire_surface: None,
			wire_edges: None,
			vertex_markers: None,
			shadows: Some(shadows),
		})
	}

	pub(crate) fn prepare_editor_geometry(
		&mut self,
		device: &Device,
		camera_layout: vk::DescriptorSetLayout,
		texture_layout: vk::DescriptorSetLayout,
		wireframe: bool,
		markers: bool,
	) -> Result<(), String> {
		let color = self.color_format;
		let depth = self.depth_format;
		let create = |pass, skinned, double_sided| {
			VulkanGraphicsPipeline::new_kind(
				device,
				color,
				depth,
				camera_layout,
				texture_layout,
				PipelineOptions {
					double_sided,
					overlay: false,
					skinned,
					pass,
				},
			)
		};
		if wireframe && self.wire_edges.is_none() {
			let surface = EditorMeshPipelines {
				ordinary: create(MeshPass::WireSurface, false, false)?,
				skinned: create(MeshPass::WireSurface, true, false)?,
				double_sided: Some((
					create(MeshPass::WireSurface, false, true)?,
					create(MeshPass::WireSurface, true, true)?,
				)),
			};
			let edges = EditorMeshPipelines {
				ordinary: create(MeshPass::WireEdges, false, true)?,
				skinned: create(MeshPass::WireEdges, true, true)?,
				double_sided: None,
			};
			self.wire_surface = Some(surface);
			self.wire_edges = Some(edges);
		}
		if markers && self.vertex_markers.is_none() {
			self.vertex_markers = Some(EditorMeshPipelines {
				ordinary: create(MeshPass::VertexMarkers, false, true)?,
				skinned: create(MeshPass::VertexMarkers, true, true)?,
				double_sided: None,
			});
		}
		Ok(())
	}

	pub(crate) fn for_mesh_pass(
		&self,
		pass: MeshPass,
		skinned: bool,
		double_sided: bool,
		overlay: bool,
	) -> &VulkanGraphicsPipeline {
		match pass {
			MeshPass::Shadow => self
				.shadows
				.as_ref()
				.expect("shadow pipelines")
				.select(skinned, double_sided),
			MeshPass::Surface if skinned => self.for_skin(double_sided),
			MeshPass::Surface => self.for_object(double_sided, overlay),
			MeshPass::WireSurface => self
				.wire_surface
				.as_ref()
				.expect("prepared wireframe surface")
				.select(skinned, double_sided),
			MeshPass::WireEdges => self
				.wire_edges
				.as_ref()
				.expect("prepared wireframe edges")
				.select(skinned, double_sided),
			MeshPass::VertexMarkers => self
				.vertex_markers
				.as_ref()
				.expect("prepared vertex markers")
				.select(skinned, double_sided),
		}
	}

	pub(crate) fn has_shadows(&self) -> bool {
		self.shadows.is_some()
	}

	pub(crate) fn supports_formats(&self, color: vk::Format, depth: vk::Format) -> bool {
		self.color_format == color && self.depth_format == depth
	}

	pub fn for_skin(&self, double_sided: bool) -> &VulkanGraphicsPipeline {
		if double_sided {
			&self.skinned_double
		} else {
			&self.skinned_single
		}
	}
	pub fn for_material(&self, double_sided: bool) -> &VulkanGraphicsPipeline {
		if double_sided {
			&self.double_sided
		} else {
			&self.single_sided
		}
	}
	pub fn for_object(&self, double_sided: bool, overlay: bool) -> &VulkanGraphicsPipeline {
		if overlay {
			&self.overlay
		} else {
			self.for_material(double_sided)
		}
	}
}

pub struct VulkanGraphicsPipeline {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,
	pipeline: vk::Pipeline,
	layout: vk::PipelineLayout,
	_palette_layout: Option<super::skinning::PaletteLayout>,
}

/// Opaque-material pipeline variants with identical descriptor/push layouts.
/// Recreate the whole set when attachment formats change with the swapchain.
impl VulkanGraphicsPipeline {
	pub fn new(
		device: &Device,
		color_format: vk::Format,
		depth_format: vk::Format,

		camera_layout: vk::DescriptorSetLayout,

		texture_layout: vk::DescriptorSetLayout,
		double_sided: bool,
		overlay: bool,
	) -> Result<Self, String> {
		Self::new_kind(
			device,
			color_format,
			depth_format,
			camera_layout,
			texture_layout,
			PipelineOptions {
				double_sided,
				overlay,
				skinned: false,
				pass: MeshPass::Surface,
			},
		)
	}
	fn new_kind(
		device: &Device,
		color_format: vk::Format,
		depth_format: vk::Format,
		camera_layout: vk::DescriptorSetLayout,
		texture_layout: vk::DescriptorSetLayout,
		options: PipelineOptions,
	) -> Result<Self, String> {
		let PipelineOptions {
			double_sided,
			overlay,
			skinned,
			pass,
		} = options;
		let marker = pass == MeshPass::VertexMarkers;
		let shadow = pass == MeshPass::Shadow;
		let palette_layout = super::skinning::PaletteLayout::new(device)
			.map_err(|e| format!("Skin descriptor layout: {e:?}"))?;
		let vertex_shader = create_shader_module(
			device,
			match (skinned, marker, shadow) {
				(true, _, true) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/skinned-shadow.vert.spv"))
				}
				(false, _, true) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/triangle-shadow.vert.spv"))
				}
				(true, true, false) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/skinned-markers.vert.spv"))
				}
				(false, true, false) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/triangle-markers.vert.spv"))
				}
				(true, false, false) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/skinned.vert.spv"))
				}
				(false, false, false) => {
					include_bytes!(concat!(env!("OUT_DIR"), "/triangle.vert.spv"))
				}
			},
		)?;
		let _vertex_metrics = crate::metrics::object(
			device,
			"shader_module",
			vertex_shader,
			"Vertex shader",
			"SPIR-V",
		);

		let fragment_shader = match create_shader_module(
			device,
			if shadow {
				include_bytes!(concat!(env!("OUT_DIR"), "/shadow.frag.spv"))
			} else {
				include_bytes!(concat!(env!("OUT_DIR"), "/triangle.frag.spv"))
			},
		) {
			Ok(shader) => shader,

			Err(error) => {
				unsafe {
					device.destroy_shader_module(vertex_shader, None);
				}

				return Err(error);
			}
		};

		let _fragment_metrics = crate::metrics::object(
			device,
			"shader_module",
			fragment_shader,
			"Fragment shader",
			"SPIR-V",
		);
		let push_constant_ranges = [vk::PushConstantRange::default()
			.stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
			.offset(ModelPushConstants::OFFSET)
			.size(ModelPushConstants::SIZE)];

		let descriptor_set_layouts = [
			camera_layout,
			texture_layout,
			palette_layout.raw,
			texture_layout,
		];

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

		let input_rate = if marker {
			vk::VertexInputRate::INSTANCE
		} else {
			vk::VertexInputRate::VERTEX
		};
		let mesh_binding = Vertex::binding_description().input_rate(input_rate);
		let vertex_binding_descriptions = if skinned {
			vec![
				mesh_binding,
				vk::VertexInputBindingDescription {
					binding: 2,
					stride: size_of::<super::mesh::SkinInfluence>() as u32,
					input_rate,
				},
			]
		} else if marker {
			vec![mesh_binding]
		} else {
			vec![mesh_binding, InstanceData::binding_description()]
		};
		let mut vertex_attribute_descriptions = Vec::from(Vertex::attribute_descriptions());
		if marker {
			vertex_attribute_descriptions.retain(|attribute| attribute.location == 0);
		} else if shadow {
			vertex_attribute_descriptions.retain(|attribute| attribute.location <= 1);
		}
		if skinned {
			vertex_attribute_descriptions.extend([
				vk::VertexInputAttributeDescription {
					location: 3,
					binding: 2,
					format: vk::Format::R16G16B16A16_UINT,
					offset: 0,
				},
				vk::VertexInputAttributeDescription {
					location: 4,
					binding: 2,
					format: vk::Format::R32G32B32A32_SFLOAT,
					offset: 8,
				},
			]);
		} else if !marker {
			vertex_attribute_descriptions.extend(InstanceData::attribute_descriptions());
		}

		let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
			.vertex_binding_descriptions(&vertex_binding_descriptions)
			.vertex_attribute_descriptions(&vertex_attribute_descriptions);

		let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
			.topology(if pass == MeshPass::WireEdges {
				vk::PrimitiveTopology::LINE_LIST
			} else {
				vk::PrimitiveTopology::TRIANGLE_LIST
			})
			.primitive_restart_enable(false);

		let viewport_state = vk::PipelineViewportStateCreateInfo::default()
			.viewport_count(1)
			.scissor_count(1);

		let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
			.depth_clamp_enable(false)
			.rasterizer_discard_enable(false)
			.polygon_mode(vk::PolygonMode::FILL)
			.cull_mode(if double_sided {
				vk::CullModeFlags::NONE
			} else {
				vk::CullModeFlags::BACK
			})
			.front_face(vk::FrontFace::CLOCKWISE)
			.line_width(1.0)
			.depth_bias_enable(pass == MeshPass::WireEdges || shadow)
			.depth_bias_constant_factor(if shadow { 1.0 } else { -1.0 })
			.depth_bias_slope_factor(if shadow { 1.0 } else { 0.0 });

		let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
			.rasterization_samples(vk::SampleCountFlags::TYPE_1);

		let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
			.depth_test_enable(!overlay)
			.depth_write_enable(!overlay && !pass.depth_read_only())
			.depth_compare_op(if pass.depth_read_only() {
				vk::CompareOp::LESS_OR_EQUAL
			} else {
				vk::CompareOp::LESS
			})
			.depth_bounds_test_enable(false)
			.stencil_test_enable(false);

		let color_blend_attachment = [vk::PipelineColorBlendAttachmentState {
			blend_enable: vk::FALSE,

			color_write_mask: vk::ColorComponentFlags::RGBA,

			..Default::default()
		}];

		let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
			.attachments(if shadow { &[] } else { &color_blend_attachment });

		let dynamic_states = [
			vk::DynamicState::VIEWPORT,
			vk::DynamicState::SCISSOR,
			vk::DynamicState::FRONT_FACE,
		];

		let dynamic_state =
			vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

		let color_formats = [color_format];

		let mut rendering_info = vk::PipelineRenderingCreateInfo::default()
			.color_attachment_formats(if shadow { &[] } else { &color_formats })
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
			_metrics: vec![
				crate::metrics::object(
					device,
					"pipeline",
					pipelines[0],
					"Graphics pipeline",
					&format!("double_sided={double_sided}, overlay={overlay}"),
				),
				crate::metrics::object(
					device,
					"pipeline_layout",
					layout,
					"Graphics pipeline layout",
					"",
				),
			],
			layout,
			_palette_layout: Some(palette_layout),
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
