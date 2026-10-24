use super::*;

#[test]
fn incompatible_discrete_gpus_do_not_outrank_compatible_integrated_gpus() {
	let mut discrete = vk::PhysicalDeviceProperties {
		device_type: vk::PhysicalDeviceType::DISCRETE_GPU,
		api_version: vk::API_VERSION_1_2,
		..Default::default()
	};
	assert_eq!(presentation_device_score(&discrete, vk::TRUE), None);
	discrete.api_version = vk::API_VERSION_1_3;
	assert_eq!(presentation_device_score(&discrete, vk::FALSE), None);

	let integrated = vk::PhysicalDeviceProperties {
		device_type: vk::PhysicalDeviceType::INTEGRATED_GPU,
		api_version: vk::API_VERSION_1_3,
		..Default::default()
	};
	assert_eq!(presentation_device_score(&integrated, vk::TRUE), Some(500));
	assert_eq!(presentation_device_score(&discrete, vk::TRUE), Some(1000));
}
