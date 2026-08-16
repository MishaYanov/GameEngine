use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use ash::{
	khr::swapchain as khr_swapchain,
	vk,
	Device,
	Instance,
};

use super::{
	VulkanDevice,
	VulkanSurface,
	VulkanSurfaceError,
};

pub struct VulkanSwapchain {
	loader: khr_swapchain::Device,
	device: Device,
	swapchain: vk::SwapchainKHR,
	images: Vec<vk::Image>,
	image_views: Vec<vk::ImageView>,
	format: vk::Format,
	extent: vk::Extent2D,
}

#[derive(Debug)]
pub enum VulkanSwapchainError {
	Vulkan(vk::Result),
	Surface(VulkanSurfaceError),
	NoSurfaceFormats,
	NoPresentModes,
	NoCompositeAlphaMode,
	UnsupportedImageUsage,
}

impl Display for VulkanSwapchainError {
	fn fmt(
		&self,
		formatter: &mut Formatter<'_>,
	) -> std::fmt::Result {
		match self {
			Self::Vulkan(error) => {
				write!(
					formatter,
					"Vulkan swapchain error: {error:?}",
				)
			}

			Self::Surface(error) => {
				write!(
					formatter,
					"Vulkan surface error: {error}",
				)
			}

			Self::NoSurfaceFormats => {
				write!(
					formatter,
					"surface exposes no usable formats",
				)
			}

			Self::NoPresentModes => {
				write!(
					formatter,
					"surface exposes no presentation modes",
				)
			}

			Self::NoCompositeAlphaMode => {
				write!(
					formatter,
					"surface exposes no supported composite alpha mode",
				)
			}

			Self::UnsupportedImageUsage => {
				write!(
					formatter,
					"surface does not support the required swapchain image usage",
				)
			}
		}
	}
}

impl Error for VulkanSwapchainError {}

impl From<vk::Result> for VulkanSwapchainError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl From<VulkanSurfaceError> for VulkanSwapchainError {
	fn from(value: VulkanSurfaceError) -> Self {
		Self::Surface(value)
	}
}

impl VulkanSwapchain {
	pub fn new(
		instance: &Instance,
		device: &VulkanDevice,
		surface: &VulkanSurface,
		width: u32,
		height: u32,
	) -> Result<Self, VulkanSwapchainError> {
		Self::create(
			instance,
			device,
			surface,
			width,
			height,
			vk::SwapchainKHR::null(),
		)
	}

	pub fn recreate(
		instance: &Instance,
		device: &VulkanDevice,
		surface: &VulkanSurface,
		width: u32,
		height: u32,
		old_swapchain: &VulkanSwapchain,
	) -> Result<Self, VulkanSwapchainError> {
		Self::create(
			instance,
			device,
			surface,
			width,
			height,
			old_swapchain.raw(),
		)
	}

	fn create(
		instance: &Instance,
		device: &VulkanDevice,
		surface: &VulkanSurface,
		width: u32,
		height: u32,
		old_swapchain: vk::SwapchainKHR,
	) -> Result<Self, VulkanSwapchainError> {
		let physical_device =
			device.physical_device();

		let capabilities =
			surface.capabilities(
				physical_device,
			)?;

		let formats =
			surface.formats(
				physical_device,
			)?;

		let present_modes =
			surface.present_modes(
				physical_device,
			)?;

		if formats.is_empty() {
			return Err(
				VulkanSwapchainError::NoSurfaceFormats,
			);
		}

		if present_modes.is_empty() {
			return Err(
				VulkanSwapchainError::NoPresentModes,
			);
		}

		let surface_format =
			choose_surface_format(&formats);

		let present_mode =
			choose_present_mode(&present_modes);

		let extent =
			choose_extent(
				&capabilities,
				width,
				height,
			);

		let image_count =
			choose_image_count(
				&capabilities,
			);

		let composite_alpha =
			choose_composite_alpha(
				capabilities
					.supported_composite_alpha,
			)
				.ok_or(
					VulkanSwapchainError::
					NoCompositeAlphaMode,
				)?;

		let required_usage =
			vk::ImageUsageFlags::COLOR_ATTACHMENT
				| vk::ImageUsageFlags::TRANSFER_DST;

		if !capabilities
			.supported_usage_flags
			.contains(required_usage)
		{
			return Err(
				VulkanSwapchainError::
				UnsupportedImageUsage,
			);
		}

		let queue_families =
			device.queue_families();

		let queue_family_indices = [
			queue_families.graphics,
			queue_families.present,
		];

		let mut create_info =
			vk::SwapchainCreateInfoKHR::default()
				.surface(surface.raw())
				.min_image_count(image_count)
				.image_format(
					surface_format.format,
				)
				.image_color_space(
					surface_format.color_space,
				)
				.image_extent(extent)
				.image_array_layers(1)
				.image_usage(required_usage)
				.pre_transform(
					capabilities
						.current_transform,
				)
				.composite_alpha(
					composite_alpha,
				)
				.present_mode(
					present_mode,
				)
				.clipped(true)
				.old_swapchain(
					old_swapchain,
				);

		if queue_families.graphics
			!= queue_families.present
		{
			create_info =
				create_info
					.image_sharing_mode(
						vk::SharingMode::CONCURRENT,
					)
					.queue_family_indices(
						&queue_family_indices,
					);
		} else {
			create_info =
				create_info
					.image_sharing_mode(
						vk::SharingMode::EXCLUSIVE,
					);
		}

		let loader =
			khr_swapchain::Device::new(
				instance,
				device.raw(),
			);

		let swapchain = unsafe {
			loader.create_swapchain(
				&create_info,
				None,
			)?
		};

		let images = unsafe {
			loader.get_swapchain_images(
				swapchain,
			)?
		};

		let ash_device =
			device.raw().clone();

		let image_views =
			create_image_views(
				&ash_device,
				&images,
				surface_format.format,
			)?;

		Ok(Self {
			loader,
			device: ash_device,
			swapchain,
			images,
			image_views,
			format: surface_format.format,
			extent,
		})
	}

	pub fn acquire_next_image(
		&self,
		timeout: u64,
		semaphore: vk::Semaphore,
		fence: vk::Fence,
	) -> Result<(u32, bool), vk::Result> {
		unsafe {
			self.loader.acquire_next_image(
				self.swapchain,
				timeout,
				semaphore,
				fence,
			)
		}
	}

	pub fn present(
		&self,
		queue: vk::Queue,
		wait_semaphores: &[vk::Semaphore],
		image_index: u32,
	) -> Result<bool, vk::Result> {
		let swapchains = [
			self.swapchain,
		];

		let image_indices = [
			image_index,
		];

		let present_info =
			vk::PresentInfoKHR::default()
				.wait_semaphores(
					wait_semaphores,
				)
				.swapchains(
					&swapchains,
				)
				.image_indices(
					&image_indices,
				);

		unsafe {
			self.loader.queue_present(
				queue,
				&present_info,
			)
		}
	}

	pub fn raw(
		&self,
	) -> vk::SwapchainKHR {
		self.swapchain
	}

	pub fn images(
		&self,
	) -> &[vk::Image] {
		&self.images
	}

	pub fn image_views(
		&self,
	) -> &[vk::ImageView] {
		&self.image_views
	}

	pub fn format(
		&self,
	) -> vk::Format {
		self.format
	}

	pub fn extent(
		&self,
	) -> vk::Extent2D {
		self.extent
	}
}

impl Drop for VulkanSwapchain {
	fn drop(&mut self) {
		unsafe {
			for image_view in
				self.image_views.drain(..)
			{
				self.device
					.destroy_image_view(
						image_view,
						None,
					);
			}

			self.loader
				.destroy_swapchain(
					self.swapchain,
					None,
				);
		}
	}
}

fn create_image_views(
	device: &Device,
	images: &[vk::Image],
	format: vk::Format,
) -> Result<
	Vec<vk::ImageView>,
	VulkanSwapchainError,
> {
	let mut views =
		Vec::with_capacity(
			images.len(),
		);

	for &image in images {
		let subresource_range =
			vk::ImageSubresourceRange::default()
				.aspect_mask(
					vk::ImageAspectFlags::COLOR,
				)
				.base_mip_level(0)
				.level_count(1)
				.base_array_layer(0)
				.layer_count(1);

		let create_info =
			vk::ImageViewCreateInfo::default()
				.image(image)
				.view_type(
					vk::ImageViewType::TYPE_2D,
				)
				.format(format)
				.subresource_range(
					subresource_range,
				);

		let view = unsafe {
			device.create_image_view(
				&create_info,
				None,
			)?
		};

		views.push(view);
	}

	Ok(views)
}

fn choose_surface_format(
	formats: &[vk::SurfaceFormatKHR],
) -> vk::SurfaceFormatKHR {
	formats
		.iter()
		.copied()
		.find(|format| {
			format.format
				== vk::Format::
			B8G8R8A8_SRGB
				&& format.color_space
				== vk::ColorSpaceKHR::
			SRGB_NONLINEAR
		})
		.unwrap_or(formats[0])
}

fn choose_present_mode(
	modes: &[vk::PresentModeKHR],
) -> vk::PresentModeKHR {
	modes
		.iter()
		.copied()
		.find(|mode| {
			*mode
				== vk::PresentModeKHR::
			MAILBOX
		})
		.unwrap_or(
			vk::PresentModeKHR::FIFO,
		)
}

fn choose_extent(
	capabilities:
	&vk::SurfaceCapabilitiesKHR,
	width: u32,
	height: u32,
) -> vk::Extent2D {
	/*
	 * Some platforms choose the extent
	 * for us.
	 */
	if capabilities
		.current_extent
		.width
		!= u32::MAX
	{
		return capabilities
			.current_extent;
	}

	vk::Extent2D {
		width: width.clamp(
			capabilities
				.min_image_extent
				.width,

			capabilities
				.max_image_extent
				.width,
		),

		height: height.clamp(
			capabilities
				.min_image_extent
				.height,

			capabilities
				.max_image_extent
				.height,
		),
	}
}

fn choose_image_count(
	capabilities:
	&vk::SurfaceCapabilitiesKHR,
) -> u32 {
	let mut count =
		capabilities
			.min_image_count
			+ 1;

	/*
	 * max_image_count == 0 means
	 * there is no explicit maximum.
	 */
	if capabilities.max_image_count > 0
		&& count
		> capabilities.max_image_count
	{
		count =
			capabilities.max_image_count;
	}

	count
}

fn choose_composite_alpha(
	supported:
	vk::CompositeAlphaFlagsKHR,
) -> Option<
	vk::CompositeAlphaFlagsKHR,
> {
	[
		vk::CompositeAlphaFlagsKHR::OPAQUE,
		vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
		vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
		vk::CompositeAlphaFlagsKHR::INHERIT,
	]
		.into_iter()
		.find(|mode| {
			supported.contains(*mode)
		})
}