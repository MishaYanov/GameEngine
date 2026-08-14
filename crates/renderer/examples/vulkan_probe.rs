use renderer::vulkan::{
	format_api_version,
	VulkanDevice,
	VulkanInstance,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("Initializing Vulkan...");

	let vulkan = VulkanInstance::new()?;

	println!("Vulkan instance created.");

	let devices = vulkan.physical_devices()?;

	println!("Found {} Vulkan device(s):", devices.len());

	for (index, device) in devices.iter().enumerate() {
		println!();
		println!("GPU #{index}");
		println!("  Name:       {}", device.name);
		println!("  Type:       {:?}", device.device_type);
		println!(
			"  Vulkan API: {}",
			format_api_version(device.api_version)
		);
		println!("  Vendor ID:  {:#06x}", device.vendor_id);
		println!("  Device ID:  {:#06x}", device.device_id);
	}

	println!();
	println!("Selecting Vulkan device...");

	let device = VulkanDevice::new(vulkan.raw())?;

	println!("Logical Vulkan device created.");
	println!(
		"Graphics queue family: {}",
		device.graphics_queue_family()
	);

	Ok(())
}