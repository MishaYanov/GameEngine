use std::{
	error::Error,
	ffi::{c_void, CStr},
	fmt::{Display, Formatter},
};
use std::ffi::c_char;
use ash::{
	ext::debug_utils,
	vk,
	Entry,
	Instance,
};

use raw_window_handle::RawDisplayHandle;

const APPLICATION_NAME: &CStr = c"Game Engine";
const ENGINE_NAME: &CStr = c"Game Engine";

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

const TARGET_API_VERSION: u32 = vk::API_VERSION_1_3;

const VALIDATION_ENABLED: bool = cfg!(debug_assertions);

pub struct VulkanInstance {
	entry: Entry,
	instance: Instance,

	debug_messenger: Option<DebugMessenger>,
}

struct DebugMessenger {
	loader: debug_utils::Instance,
	messenger: vk::DebugUtilsMessengerEXT,
}

#[derive(Debug)]
pub struct PhysicalDeviceInfo {
	pub name: String,
	pub device_type: vk::PhysicalDeviceType,
	pub api_version: u32,
	pub driver_version: u32,
	pub vendor_id: u32,
	pub device_id: u32,
}

#[derive(Debug)]
pub enum VulkanInitError {
	Loader(ash::LoadingError),
	Vulkan(vk::Result),

	ValidationLayerUnavailable,

	UnsupportedApiVersion {
		required: u32,
		available: u32,
	},

	NoPhysicalDevices,
}

impl Display for VulkanInitError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Loader(error) => {
				write!(formatter, "failed to load Vulkan: {error}")
			}

			Self::Vulkan(error) => {
				write!(formatter, "Vulkan error: {error:?}")
			}

			Self::ValidationLayerUnavailable => {
				write!(
					formatter,
					"Vulkan validation layer '{}' is unavailable",
					VALIDATION_LAYER.to_string_lossy()
				)
			}

			Self::UnsupportedApiVersion {
				required,
				available,
			} => {
				write!(
					formatter,
					"Vulkan API {} required, but loader only supports {}",
					format_api_version(*required),
					format_api_version(*available)
				)
			}

			Self::NoPhysicalDevices => {
				write!(formatter, "no Vulkan-capable physical devices found")
			}
		}
	}
}

impl Error for VulkanInitError {}

impl From<ash::LoadingError> for VulkanInitError {
	fn from(value: ash::LoadingError) -> Self {
		Self::Loader(value)
	}
}

impl From<vk::Result> for VulkanInitError {
	fn from(value: vk::Result) -> Self {
		Self::Vulkan(value)
	}
}

impl VulkanInstance {
	pub fn new() -> Result<Self, VulkanInitError> {
		Self::with_extensions(&[])
	}

	pub fn for_display(
		display_handle: RawDisplayHandle,
	) -> Result<Self, VulkanInitError> {
		let extensions =
			ash_window::enumerate_required_extensions(
				display_handle,
			)?;

		Self::with_extensions(extensions)
	}

	fn with_extensions(
		required_extensions: &[*const c_char],
	) -> Result<Self, VulkanInitError> {
		let entry = unsafe {
			Entry::load()?
		};

		let loader_version = unsafe {
			entry.try_enumerate_instance_version()?
		}
			.unwrap_or(vk::API_VERSION_1_0);

		if loader_version < TARGET_API_VERSION {
			return Err(
				VulkanInitError::UnsupportedApiVersion {
					required: TARGET_API_VERSION,
					available: loader_version,
				},
			);
		}

		if VALIDATION_ENABLED
			&& !validation_layer_available(&entry)?
		{
			return Err(
				VulkanInitError::ValidationLayerUnavailable,
			);
		}

		let application_info =
			vk::ApplicationInfo::default()
				.application_name(APPLICATION_NAME)
				.application_version(
					vk::make_api_version(0, 0, 1, 0),
				)
				.engine_name(ENGINE_NAME)
				.engine_version(
					vk::make_api_version(0, 0, 1, 0),
				)
				.api_version(TARGET_API_VERSION);

		let validation_layers = [
			VALIDATION_LAYER.as_ptr(),
		];

		let mut extensions =
			required_extensions.to_vec();

		if VALIDATION_ENABLED {
			extensions.push(
				debug_utils::NAME.as_ptr(),
			);
		}

		let mut debug_create_info =
			create_debug_messenger_info();

		let mut instance_create_info =
			vk::InstanceCreateInfo::default()
				.application_info(&application_info)
				.enabled_extension_names(&extensions);

		if VALIDATION_ENABLED {
			instance_create_info =
				instance_create_info
					.enabled_layer_names(
						&validation_layers,
					);

			instance_create_info =
				instance_create_info
					.push_next(
						&mut debug_create_info,
					);
		}

		let instance = unsafe {
			entry.create_instance(
				&instance_create_info,
				None,
			)?
		};

		let debug_messenger =
			if VALIDATION_ENABLED {
				Some(
					create_debug_messenger(
						&entry,
						&instance,
					)?,
				)
			} else {
				None
			};

		Ok(Self {
			entry,
			instance,
			debug_messenger,
		})
	}

	pub fn raw(&self) -> &Instance {
		&self.instance
	}

	pub fn entry(&self) -> &Entry {
		&self.entry
	}

	pub fn physical_devices(
		&self,
	) -> Result<Vec<PhysicalDeviceInfo>, VulkanInitError> {
		let devices = unsafe {
			self.instance.enumerate_physical_devices()?
		};

		if devices.is_empty() {
			return Err(VulkanInitError::NoPhysicalDevices);
		}

		let mut result = Vec::with_capacity(devices.len());

		for device in devices {
			let properties = unsafe {
				self.instance
					.get_physical_device_properties(device)
			};

			let name = unsafe {
				CStr::from_ptr(properties.device_name.as_ptr())
			}
				.to_string_lossy()
				.into_owned();

			result.push(PhysicalDeviceInfo {
				name,
				device_type: properties.device_type,
				api_version: properties.api_version,
				driver_version: properties.driver_version,
				vendor_id: properties.vendor_id,
				device_id: properties.device_id,
			});
		}

		Ok(result)
	}
}

impl Drop for VulkanInstance {
	fn drop(&mut self) {
		unsafe {
			if let Some(debug) = &self.debug_messenger {
				debug.loader.destroy_debug_utils_messenger(
					debug.messenger,
					None,
				);
			}

			self.instance.destroy_instance(None);
		}
	}
}

fn validation_layer_available(
	entry: &Entry,
) -> Result<bool, VulkanInitError> {
	let layers = unsafe {
		entry.enumerate_instance_layer_properties()?
	};

	Ok(
		layers
			.iter()
			.any(|layer| {
				let name = unsafe {
					CStr::from_ptr(layer.layer_name.as_ptr())
				};

				name == VALIDATION_LAYER
			})
	)
}

fn create_debug_messenger(
	entry: &Entry,
	instance: &Instance,
) -> Result<DebugMessenger, VulkanInitError> {
	let loader = debug_utils::Instance::new(
		entry,
		instance,
	);

	let create_info = create_debug_messenger_info();

	let messenger = unsafe {
		loader.create_debug_utils_messenger(
			&create_info,
			None,
		)?
	};

	Ok(DebugMessenger {
		loader,
		messenger,
	})
}

fn create_debug_messenger_info(
) -> vk::DebugUtilsMessengerCreateInfoEXT<'static> {
	vk::DebugUtilsMessengerCreateInfoEXT::default()
		.message_severity(
			vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
				| vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
				| vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
		)
		.message_type(
			vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
				| vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
				| vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
		)
		.pfn_user_callback(Some(vulkan_debug_callback))
}

unsafe extern "system" fn vulkan_debug_callback(
	severity: vk::DebugUtilsMessageSeverityFlagsEXT,
	message_type: vk::DebugUtilsMessageTypeFlagsEXT,
	callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
	_user_data: *mut c_void,
) -> vk::Bool32 {
	if callback_data.is_null() {
		return vk::FALSE;
	}

	let callback_data = unsafe {
		&*callback_data
	};

	let message = if callback_data.p_message.is_null() {
		"<no message>".into()
	} else {
		unsafe {
			CStr::from_ptr(callback_data.p_message)
		}
			.to_string_lossy()
	};

	eprintln!(
		"[Vulkan][{severity:?}][{message_type:?}] {message}"
	);

	vk::FALSE
}

pub fn format_api_version(version: u32) -> String {
	format!(
		"{}.{}.{}",
		vk::api_version_major(version),
		vk::api_version_minor(version),
		vk::api_version_patch(version),
	)
}