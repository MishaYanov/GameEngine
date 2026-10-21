use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneData {
    pub schema_version: u32,
    pub entities: Vec<EntityData>,
    pub camera: CameraData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityData {
    pub name: String,
    pub model_path: String,
    pub transform: TransformData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraData {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub vertical_fov: f32,

    #[serde(rename = "near")]
    pub near_plane: f32,

    #[serde(rename = "far")]
    pub far_plane: f32,
}

impl SceneData {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "Unsupported schema version: {}",
                self.schema_version
            ));
        }

        for (index, entity) in self.entities.iter().enumerate() {
            let context = format!("entity {index} ({:?})", entity.name);

            if entity.name.trim().is_empty() {
                return Err(format!("{context}: name cannot be empty or whitespace"));
            }

            let path = entity.model_path.as_str();

            if path.trim().is_empty() {
                return Err(format!("{context}: model path cannot be empty"));
            }

            // Portable asset-relative paths use "/" separators.
            if path.starts_with('/')
                || path.contains('\\')
                || path.contains(':')
                || path.split('/').any(|component| component == "..")
            {
                return Err(format!(
                    "{context}: model path must be asset-relative, use '/' \
                 separators, and contain no drive prefix or '..' components"
                ));
            }

            let transform = &entity.transform;

            if !transform
                .position
                .iter()
                .chain(transform.rotation.iter())
                .chain(transform.scale.iter())
                .all(|value| value.is_finite())
            {
                return Err(format!("{context}: transform values must all be finite"));
            }
        }

        let camera = &self.camera;
        let camera_scalars = [
            camera.yaw,
            camera.pitch,
            camera.vertical_fov,
            camera.near_plane,
            camera.far_plane,
        ];

        if !camera
            .position
            .iter()
            .chain(camera_scalars.iter())
            .all(|value| value.is_finite())
        {
            return Err("Camera values must all be finite".to_string());
        }

        let pitch_limit = 89.0_f32.to_radians();

        if !(-pitch_limit..=pitch_limit).contains(&camera.pitch) {
            return Err("Camera pitch must be within ±89 degrees".to_string());
        }

        if camera.vertical_fov <= 0.0 || camera.vertical_fov >= std::f32::consts::PI {
            return Err("Camera vertical FOV must be between 0 and PI radians".to_string());
        }

        if camera.near_plane <= 0.0 {
            return Err("Camera near plane must be greater than zero".to_string());
        }

        if camera.far_plane <= camera.near_plane {
            return Err("Camera far plane must be greater than the near plane".to_string());
        }

        Ok(())
    }

    pub fn from_json(source: &str) -> Result<Self, String> {
        let scene = serde_json::from_str::<Self>(source)
            .map_err(|error| format!("Invalid scene JSON: {error}"))?;

        scene.validate()?;

        Ok(scene)
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;

        serde_json::to_string_pretty(self)
            .map_err(|error| format!("Failed to serialize scene: {error}"))
    }
}

#[cfg(test)]
#[path = "tests_scene_data.rs"]
mod tests;
