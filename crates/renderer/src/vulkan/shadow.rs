//! First directional shadow map allocation, camera fitting, and command recording.

use ash::{Device, Instance, vk};
use glam::{Mat4, Vec2, Vec3, Vec4};

use super::{
	CameraUniform, LightingUniform, VulkanDepthBuffer, VulkanDepthBufferError, VulkanDevice,
};

const MAP_SIZE: u32 = 2048;
const SHADOW_DISTANCE: f32 = 150.0;
const OCCLUDER_DEPTH: f32 = 140.0;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShadowUniform {
	light_view_projection: [f32; 16],
	// x = one-based light index (zero disables); yz = inverse texture extent.
	light_info: [f32; 4],
}

impl ShadowUniform {
	pub(crate) fn fit(
		camera: &CameraUniform,
		light_index: usize,
		light_direction: Vec3,
		size: u32,
	) -> Option<Self> {
		let inverse_camera = Mat4::from_cols_array(&camera.view_projection).inverse();
		if !inverse_camera.is_finite() {
			return None;
		}

		let up = if light_direction.y.abs() > 0.96 {
			Vec3::Z
		} else {
			Vec3::Y
		};
		let light_view = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, light_direction, up);
		let mut low = Vec3::splat(f32::MAX);
		let mut high = Vec3::splat(-f32::MAX);
		for x in [-1.0, 1.0] {
			for y in [-1.0, 1.0] {
				let near = inverse_camera * Vec4::new(x, y, 0.0, 1.0);
				let far = inverse_camera * Vec4::new(x, y, 1.0, 1.0);
				if near.w.abs() < 1e-7 {
					return None;
				}
				let near_point = near.truncate() / near.w;
				// Infinite projections put the far plane at homogeneous w=0.
				let far_point = if far.w.abs() < 1e-7 {
					near_point + far.truncate().try_normalize()? * SHADOW_DISTANCE
				} else {
					far.truncate() / far.w
				};
				let ray = far_point - near_point;
				let end = near_point + ray.try_normalize()? * ray.length().min(SHADOW_DISTANCE);
				for point in [near_point, end] {
					let light_point = light_view.transform_point3(point);
					low = low.min(light_point);
					high = high.max(light_point);
				}
			}
		}
		if !low.is_finite() || !high.is_finite() {
			return None;
		}
		// A small guard band keeps the PCF footprint valid near the edge.
		let half_size = ((high - low).truncate() * 0.5 + Vec2::splat(3.0))
			.ceil()
			.max(Vec2::splat(1.0));
		let texel = half_size * (2.0 / size as f32);
		let center = (((high + low) * 0.5).truncate() / texel).round() * texel;
		// Center before projecting so Vulkan's flipped Y only sees symmetric bounds.
		let recenter = Mat4::from_translation((-center).extend(0.0));
		let light_projection = glam::camera::rh::proj::vulkan::orthographic(
			-half_size.x,
			half_size.x,
			-half_size.y,
			half_size.y,
			-high.z - OCCLUDER_DEPTH,
			-low.z + 5.0,
		);
		let view_projection = light_projection * recenter * light_view;
		view_projection.is_finite().then_some(Self {
			light_view_projection: view_projection.to_cols_array(),
			light_info: [
				(light_index + 1) as f32,
				1.0 / size as f32,
				1.0 / size as f32,
				0.0,
			],
		})
	}

	pub(crate) fn view_projection(&self) -> Mat4 {
		Mat4::from_cols_array(&self.light_view_projection)
	}

	pub(crate) fn enabled(self) -> bool {
		self.light_info[0] != 0.0
	}
}

impl Default for ShadowUniform {
	fn default() -> Self {
		Self {
			light_view_projection: Mat4::IDENTITY.to_cols_array(),
			light_info: [0.0; 4],
		}
	}
}

pub(crate) struct ShadowMap {
	depth: VulkanDepthBuffer,
	device: Device,
	sampler: vk::Sampler,
}

impl ShadowMap {
	// D16_UNORM supports both sampling and depth attachments in Vulkan.
	// Nearest sampling avoids depending on linear depth filtering.
	pub(crate) const FORMAT: vk::Format = vk::Format::D16_UNORM;

	pub(crate) fn new(
		instance: &Instance,
		device: &VulkanDevice,
	) -> Result<Self, VulkanDepthBufferError> {
		let size = unsafe { instance.get_physical_device_properties(device.physical_device()) }
			.limits
			.max_image_dimension2_d
			.min(MAP_SIZE);
		let square = |size| vk::Extent2D {
			width: size,
			height: size,
		};
		let depth = VulkanDepthBuffer::sampled(instance, device, square(size), Self::FORMAT)
			.or_else(|_| {
				VulkanDepthBuffer::sampled(instance, device, square(size.min(512)), Self::FORMAT)
			})?;
		let sampler_info = vk::SamplerCreateInfo::default()
			.mag_filter(vk::Filter::NEAREST)
			.min_filter(vk::Filter::NEAREST)
			.address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
			.address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
			.address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
			.min_lod(0.0)
			.max_lod(0.0);
		let sampler = unsafe { device.raw().create_sampler(&sampler_info, None)? };
		Ok(Self {
			depth,
			device: device.raw().clone(),
			sampler,
		})
	}

	pub(crate) fn uniform(
		&self,
		camera: &CameraUniform,
		lighting: &LightingUniform,
		shading: Option<crate::EditorShadingMode>,
	) -> Option<ShadowUniform> {
		if shading.is_some_and(|mode| mode != crate::EditorShadingMode::Lit) {
			return None;
		}
		let (index, direction) = lighting.shadow_light()?;
		ShadowUniform::fit(camera, index, direction, self.depth.extent().width)
	}

	pub(crate) fn descriptor(&self) -> vk::DescriptorImageInfo {
		vk::DescriptorImageInfo::default()
			.image_view(self.depth.view())
			.sampler(self.sampler)
			.image_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)
	}

	pub(crate) fn record(&self, command: vk::CommandBuffer, draw: Option<impl FnOnce()>) {
		let access = vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;
		let depth_stages = vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
			| vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
		let shader = vk::PipelineStageFlags::FRAGMENT_SHADER;
		let read = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
		let attachment = vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL;
		let range = vk::ImageSubresourceRange::default()
			.aspect_mask(vk::ImageAspectFlags::DEPTH)
			.level_count(1)
			.layer_count(1);
		let barrier = vk::ImageMemoryBarrier::default()
			.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
			.image(self.depth.image())
			.subresource_range(range);
		let Some(draw) = draw else {
			// The caller already waited for the previous frame. No shader
			// samples the image in this branch; the descriptor layout still
			// must be valid for the surface pipeline's statically used set.
			unsafe {
				self.device.cmd_pipeline_barrier(
					command,
					vk::PipelineStageFlags::TOP_OF_PIPE,
					shader,
					vk::DependencyFlags::empty(),
					&[],
					&[],
					&[barrier
						.old_layout(vk::ImageLayout::UNDEFINED)
						.new_layout(read)
						.dst_access_mask(vk::AccessFlags::SHADER_READ)],
				);
			}
			return;
		};
		let extent = self.depth.extent();
		let region = vk::Rect2D {
			offset: vk::Offset2D::default(),
			extent,
		};
		let viewport = vk::Viewport {
			width: extent.width as f32,
			height: extent.height as f32,
			max_depth: 1.0,
			..Default::default()
		};
		let depth = vk::RenderingAttachmentInfo::default()
			.image_view(self.depth.view())
			.image_layout(attachment)
			.load_op(vk::AttachmentLoadOp::CLEAR)
			.store_op(vk::AttachmentStoreOp::STORE)
			.clear_value(vk::ClearValue {
				depth_stencil: vk::ClearDepthStencilValue {
					depth: 1.0,
					stencil: 0,
				},
			});
		let render = vk::RenderingInfo::default()
			.render_area(region)
			.layer_count(1)
			.depth_attachment(&depth);
		// SAFETY: command recording has started, no render pass is active
		// here, and the caller has waited for prior shadow image users.
		unsafe {
			self.device.cmd_pipeline_barrier(
				command,
				vk::PipelineStageFlags::TOP_OF_PIPE,
				depth_stages,
				vk::DependencyFlags::empty(),
				&[],
				&[],
				&[barrier
					.old_layout(vk::ImageLayout::UNDEFINED)
					.new_layout(attachment)
					.dst_access_mask(access)],
			);
			self.device.cmd_begin_rendering(command, &render);
			self.device.cmd_set_viewport(command, 0, &[viewport]);
			self.device.cmd_set_scissor(command, 0, &[region]);
		}
		draw();
		unsafe {
			self.device.cmd_end_rendering(command);
			self.device.cmd_pipeline_barrier(
				command,
				depth_stages,
				shader,
				vk::DependencyFlags::empty(),
				&[],
				&[],
				&[barrier
					.old_layout(attachment)
					.new_layout(read)
					.src_access_mask(access)
					.dst_access_mask(vk::AccessFlags::SHADER_READ)],
			);
		}
	}
}

impl Drop for ShadowMap {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_sampler(self.sampler, None);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn directional_fit_contains_near_visible_points() {
		let camera = CameraUniform::perspective(
			[131.0, 67.0, -84.0],
			[131.0, 66.0, -96.0],
			[0.0, 1.0, 0.0],
			1.25,
			1.7,
			0.1,
			100.0,
		);
		let uniform = ShadowUniform::fit(
			&camera,
			2,
			Vec3::new(-0.5, -0.6, -0.5).normalize(),
			MAP_SIZE,
		)
			.unwrap();
		assert!(uniform.enabled());
		assert_eq!(uniform.light_info[0], 3.0);
		let matrix = Mat4::from_cols_array(&uniform.light_view_projection);
		let projected = matrix.project_point3(Vec3::new(131.0, 66.0, -96.0));
		assert!(projected.x.abs() < 1.0 && projected.y.abs() < 1.0);
		assert!((0.0..1.0).contains(&projected.z));
	}

	#[test]
	fn invalid_camera_disables_shadow_fit() {
		let camera = CameraUniform::from_view_projection(Mat4::ZERO);
		assert!(ShadowUniform::fit(&camera, 0, -Vec3::Y, MAP_SIZE).is_none());
		assert!(!ShadowUniform::default().enabled());
	}
}
