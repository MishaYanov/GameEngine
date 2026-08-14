use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusDto {
	pub initialized: bool,
	pub renderer: String,
	pub gpu_name: String,
	pub vulkan_api: String,
}