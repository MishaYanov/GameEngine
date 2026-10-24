use super::*;
use renderer::vulkan::VulkanInitError;

#[test]
fn initialization_errors_preserve_the_root_cause() {
	let error = GraphicsProbeError::from(RendererError::from(VulkanInitError::NoPhysicalDevices));
	assert!(error.to_string().contains("renderer initialization failed"));
	let renderer = error.source().unwrap();
	assert!(renderer.to_string().contains("Vulkan instance error"));
	assert_eq!(
		renderer.source().unwrap().to_string(),
		"no Vulkan-capable physical devices found"
	);
}

#[test]
#[ignore = "requires a Vulkan 1.3 loader, GPU and debug validation layer"]
fn native_runtime_initializes_and_reports_consistent_status() {
	let engine = GraphicsProbe::new().expect("native runtime initialization");
	for _ in 0..2 {
		let status = engine.status();
		assert!(status.initialized);
		assert_eq!(status.renderer, "Vulkan");
		assert!(!status.gpu_name.trim().is_empty());
		assert_eq!(status.vulkan_api.split('.').count(), 3);
	}
}
