use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusDto {
    pub initialized: bool,
    pub renderer: String,
    pub gpu_name: String,
    pub vulkan_api: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_matches_the_frontend_contract() {
        let status = EngineStatusDto {
            initialized: true,
            renderer: "Vulkan".into(),
            gpu_name: "GPU 日本語".into(),
            vulkan_api: "1.3.0".into(),
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "initialized": true, "renderer": "Vulkan", "gpuName": "GPU 日本語", "vulkanApi": "1.3.0"
            })
        );
    }
}
