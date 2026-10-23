use crate::entity_key::EntityKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneData {
    pub schema_version: u32,
    pub bindings: BTreeMap<String, EntityKey>,
    pub entities: Vec<EntityData>,
    pub camera: CameraData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityData {
    pub key: EntityKey,
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

#[derive(Debug)]
pub struct ParsedScene {
    pub data: SceneData,
    pub source_schema_version: u32,
}

#[derive(Deserialize)]
struct VersionHeader {
    schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneDataV1 {
    schema_version: u32,
    entities: Vec<EntityDataV1>,
    camera: CameraData,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityDataV1 {
    name: String,
    model_path: String,
    transform: TransformData,
}

impl SceneDataV1 {
    fn migrate(self) -> Result<SceneData, String> {
        let entities = self
            .entities
            .into_iter()
            .enumerate()
            .map(|(index, entity)| {
                Ok(EntityData {
                    key: EntityKey::new(format!("legacy-entity-{index}"))?,
                    name: entity.name,
                    model_path: entity.model_path,
                    transform: entity.transform,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        Ok(SceneData {
            schema_version: CURRENT_SCHEMA_VERSION,
            bindings: BTreeMap::new(),
            entities,
            camera: self.camera,
        })
    }
}

pub fn parse_json(source: &str) -> Result<ParsedScene, String> {
    let header = serde_json::from_str::<VersionHeader>(source)
        .map_err(|error| format!("Invalid scene JSON: {error}"))?;

    let parsed = match header.schema_version {
        1 => {
            let legacy = serde_json::from_str::<SceneDataV1>(source)
                .map_err(|error| format!("Invalid version-1 scene JSON: {error}"))?;
            ParsedScene {
                source_schema_version: legacy.schema_version,
                data: legacy.migrate()?,
            }
        }
        CURRENT_SCHEMA_VERSION => ParsedScene {
            source_schema_version: header.schema_version,
            data: serde_json::from_str::<SceneData>(source)
                .map_err(|error| format!("Invalid version-2 scene JSON: {error}"))?,
        },
        version => return Err(format!("Unsupported schema version: {version}")),
    };

    parsed.data.validate()?;
    Ok(parsed)
}

impl SceneData {
    pub fn validate(&self) -> Result<(), String> {
        let mut entity_keys = HashSet::new();

        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported schema version: {}",
                self.schema_version
            ));
        }

        for (index, entity) in self.entities.iter().enumerate() {
            let context = format!("entity {index} ({:?})", entity.name);

            // Derived deserialization does not call EntityKey::new.
            entity
                .key
                .validate()
                .map_err(|error| format!("{context}: {error}"))?;
            if !entity_keys.insert(entity.key.as_str()) {
                return Err(format!(
                    "{context}: duplicate entity key {:?}",
                    entity.key.as_str()
                ));
            }

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

        for (name, key) in &self.bindings {
            if name.trim().is_empty() {
                return Err("Binding name cannot be empty or whitespace".to_string());
            }
            key.validate()
                .map_err(|error| format!("Binding {name:?}: {error}"))?;
            if !entity_keys.contains(key.as_str()) {
                return Err(format!(
                    "Binding {name:?} targets missing entity key {:?}",
                    key.as_str()
                ));
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
        parse_json(source).map(|parsed| parsed.data)
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
