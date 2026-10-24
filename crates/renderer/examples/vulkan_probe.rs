use engine_logger::ResultLogExt;
use renderer::vulkan::{VulkanDevice, VulkanInstance, format_api_version};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let _logging = engine_logger::init_application("vulkan-probe")?;
	engine_logger::info!("Initializing Vulkan...");

	let vulkan = VulkanInstance::new().log_error("initialize Vulkan probe instance")?;

	engine_logger::info!("Vulkan instance created.");

	let devices = vulkan
		.physical_devices()
		.log_error("enumerate Vulkan devices")?;

	engine_logger::info!("Found {} Vulkan device(s):", devices.len());

	for (index, device) in devices.iter().enumerate() {
		engine_logger::info!("");
		engine_logger::info!("GPU #{index}");
		engine_logger::info!("  Name:       {}", device.name);
		engine_logger::info!("  Type:       {:?}", device.device_type);
		engine_logger::info!("  Vulkan API: {}", format_api_version(device.api_version));
		engine_logger::info!("  Vendor ID:  {:#06x}", device.vendor_id);
		engine_logger::info!("  Device ID:  {:#06x}", device.device_id);
	}

	engine_logger::info!("");
	engine_logger::info!("Selecting Vulkan device...");

	let device = VulkanDevice::new(vulkan.raw()).log_error("select Vulkan device")?;

	engine_logger::info!("Logical Vulkan device created.");
	engine_logger::info!(
		"Graphics queue family: {}",
		device.queue_families().graphics
	);

	Ok(())
}
