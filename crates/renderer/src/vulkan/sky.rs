//! Single-draw analytic background. It writes color before meshes and leaves depth untouched.
use ash::{Device, util::read_spv, vk};
use glam::{Mat4, Vec3};
use std::{io::Cursor, mem::size_of};

use super::CameraUniform;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyGradient {
	pub zenith: [f32; 3],
	pub horizon: [f32; 3],
}

#[repr(C)]
struct SkyPush {
	ray_x: [f32; 4],
	ray_y: [f32; 4],
	forward: [f32; 4],
	zenith: [f32; 4],
	horizon: [f32; 4],
}

pub(crate) struct SkyPass {
	_metrics: Vec<engine_logger::metrics::ResourceGuard>,
	device: Device,
	pipeline: vk::Pipeline,
	layout: vk::PipelineLayout,
	formats: (vk::Format, vk::Format),
}

struct Shader<'a> {
	device: &'a Device,
	module: vk::ShaderModule,
}

impl<'a> Shader<'a> {
	fn new(device: &'a Device, bytes: &[u8]) -> Result<Self, String> {
		let code = read_spv(&mut Cursor::new(bytes)).map_err(|error| error.to_string())?;
		let module = unsafe {
			device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
		}
		.map_err(|error| error.to_string())?;
		Ok(Self { device, module })
	}
}

impl Drop for Shader<'_> {
	fn drop(&mut self) {
		unsafe { self.device.destroy_shader_module(self.module, None) };
	}
}

impl SkyPass {
	pub(crate) fn new(
		device: &Device,
		color: vk::Format,
		depth: vk::Format,
	) -> Result<Self, String> {
		let vertex = Shader::new(
			device,
			include_bytes!(concat!(env!("OUT_DIR"), "/sky.vert.spv")),
		)?;
		let fragment = Shader::new(
			device,
			include_bytes!(concat!(env!("OUT_DIR"), "/sky.frag.spv")),
		)?;
		let range = [vk::PushConstantRange::default()
			.stage_flags(vk::ShaderStageFlags::FRAGMENT)
			.size(size_of::<SkyPush>() as u32)];
		let layout = unsafe {
			device.create_pipeline_layout(
				&vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&range),
				None,
			)
		}
		.map_err(|error| error.to_string())?;
		let mut pass = Self {
			_metrics: Vec::new(),
			device: device.clone(),
			pipeline: vk::Pipeline::null(),
			layout,
			formats: (color, depth),
		};
		let stages = [
			vk::PipelineShaderStageCreateInfo::default()
				.stage(vk::ShaderStageFlags::VERTEX)
				.module(vertex.module)
				.name(c"main"),
			vk::PipelineShaderStageCreateInfo::default()
				.stage(vk::ShaderStageFlags::FRAGMENT)
				.module(fragment.module)
				.name(c"main"),
		];
		let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
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
			.depth_test_enable(false)
			.depth_write_enable(false);
		let attachments = [vk::PipelineColorBlendAttachmentState::default()
			.color_write_mask(vk::ColorComponentFlags::RGBA)];
		let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
		let dynamic = vk::PipelineDynamicStateCreateInfo::default()
			.dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
		let formats = [color];
		let mut rendering = vk::PipelineRenderingCreateInfo::default()
			.color_attachment_formats(&formats)
			.depth_attachment_format(depth);
		let info = vk::GraphicsPipelineCreateInfo::default()
			.push_next(&mut rendering)
			.stages(&stages)
			.vertex_input_state(&vertex_input)
			.input_assembly_state(&assembly)
			.viewport_state(&viewport)
			.rasterization_state(&raster)
			.multisample_state(&multisample)
			.depth_stencil_state(&depth_state)
			.color_blend_state(&blend)
			.dynamic_state(&dynamic)
			.layout(layout);
		pass.pipeline = match unsafe {
			device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
		} {
			Ok(pipelines) => pipelines[0],
			Err((pipelines, error)) => {
				for pipeline in pipelines {
					unsafe { device.destroy_pipeline(pipeline, None) };
				}
				return Err(format!("Sky pipeline creation failed: {error}"));
			}
		};
		pass._metrics = vec![
			crate::metrics::object(
				device,
				"pipeline",
				pass.pipeline,
				"Sky background pipeline",
				"analytic background",
			),
			crate::metrics::object(
				device,
				"pipeline_layout",
				layout,
				"Sky background layout",
				"",
			),
		];
		Ok(pass)
	}

	pub(crate) fn supports_formats(&self, color: vk::Format, depth: vk::Format) -> bool {
		self.formats == (color, depth)
	}

	pub(crate) fn record(
		&self,
		command: vk::CommandBuffer,
		camera: &CameraUniform,
		gradient: SkyGradient,
	) {
		let Some((ray_x, ray_y, forward)) = world_rays(camera) else {
			return;
		};
		let push = SkyPush {
			ray_x: ray_x.extend(0.0).to_array(),
			ray_y: ray_y.extend(0.0).to_array(),
			forward: forward.extend(0.0).to_array(),
			zenith: Vec3::from_array(gradient.zenith).extend(0.0).to_array(),
			horizon: Vec3::from_array(gradient.horizon).extend(0.0).to_array(),
		};
		unsafe {
			let bytes =
				std::slice::from_raw_parts((&raw const push).cast::<u8>(), size_of::<SkyPush>());
			self.device
				.cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
			self.device.cmd_push_constants(
				command,
				self.layout,
				vk::ShaderStageFlags::FRAGMENT,
				0,
				bytes,
			);
			self.device.cmd_draw(command, 3, 1, 0, 0);
		}
	}
}

impl Drop for SkyPass {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_pipeline(self.pipeline, None);
			self.device.destroy_pipeline_layout(self.layout, None);
		}
	}
}

fn world_rays(camera: &CameraUniform) -> Option<(Vec3, Vec3, Vec3)> {
	// Ignore the translation column. The projection rows give directions scaled
	// by the perspective lens, including Vulkan's vertical axis convention.
	let transposed = Mat4::from_cols_array(&camera.view_projection).transpose();
	let forward = transposed.w_axis.truncate();
	if forward.length_squared() <= f32::EPSILON {
		return Some((
			Vec3::ZERO,
			Vec3::ZERO,
			(-transposed.z_axis.truncate()).try_normalize()?,
		));
	}
	let right = transposed.x_axis.truncate();
	let down = transposed.y_axis.truncate();
	let reciprocal = |axis: Vec3| {
		let squared = axis.length_squared();
		(squared.is_finite() && squared > f32::EPSILON).then(|| axis / squared)
	};
	Some((
		reciprocal(right)?,
		reciprocal(down)?,
		forward.try_normalize()?,
	))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn perspective_sky_rays_translate_without_parallax_and_rotate_with_camera() {
		let up = [0.0, 1.0, 0.0];
		let camera = |position, target| {
			CameraUniform::perspective(position, target, up, 1.0, 1.6, 0.1, 500.0)
		};
		let origin = world_rays(&camera([0.0; 3], [0.0, 0.0, -1.0])).unwrap();
		let moved = world_rays(&camera([200.0, 20.0, 70.0], [200.0, 20.0, 69.0])).unwrap();
		let rotated = world_rays(&camera([0.0; 3], [1.0, 0.0, 0.0])).unwrap();
		assert_eq!(origin, moved);
		assert!(origin.2.abs_diff_eq(-Vec3::Z, f32::EPSILON));
		assert!(rotated.2.abs_diff_eq(Vec3::X, f32::EPSILON));
		assert_eq!(size_of::<SkyPush>(), 80);
	}
}
