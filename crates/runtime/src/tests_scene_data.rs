use super::*;
use serde_json::{Value, json};

fn valid_data() -> SceneData {
    SceneData {
        schema_version: 1,
        entities: [-0.8, 0.8]
            .into_iter()
            .enumerate()
            .map(|(index, x)| EntityData {
                name: format!("Robot {index}"),
                model_path: "props/robot.glb".to_owned(),
                transform: TransformData {
                    position: [x, 0.0, 0.0],
                    rotation: [0.1, 0.2, 0.3],
                    scale: [0.7; 3],
                },
            })
            .collect(),
        camera: CameraData {
            position: [1.0, 2.0, 3.0],
            yaw: 0.25,
            pitch: -0.1,
            vertical_fov: 60.0_f32.to_radians(),
            near_plane: 0.1,
            far_plane: 100.0,
        },
    }
}

fn as_value() -> Value {
    serde_json::to_value(valid_data()).unwrap()
}

fn assert_rejected(data: &SceneData) {
    assert!(
        data.validate().is_err(),
        "invalid scene passed validation: {data:?}"
    );
    assert!(
        data.to_json().is_err(),
        "invalid scene was serialized: {data:?}"
    );
}

#[test]
fn two_robot_roundtrip_preserves_all_fields_and_near_far_json_names() {
    let source = valid_data();
    let json = source.to_json().unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["camera"]["near"], json!(0.1));
    assert_eq!(value["camera"]["far"], json!(100.0));
    assert!(value["camera"].get("near_plane").is_none());
    assert!(value["camera"].get("far_plane").is_none());
    let parsed = SceneData::from_json(&json).unwrap();
    assert_eq!(
        serde_json::to_value(parsed).unwrap(),
        serde_json::to_value(source).unwrap()
    );
}

#[test]
fn versions_and_unknown_fields_are_rejected_at_every_nesting_level() {
    for version in [0, 2, u32::MAX] {
        let mut data = valid_data();
        data.schema_version = version;
        assert_rejected(&data);
        assert!(SceneData::from_json(&serde_json::to_string(&data).unwrap()).is_err());
    }
    for pointer in ["", "/entities/0", "/entities/0/transform", "/camera"] {
        let mut value = as_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), json!(true));
        let error = SceneData::from_json(&value.to_string()).unwrap_err();
        assert!(error.contains("unknown field"), "{error}");
    }
}

#[test]
fn missing_fields_malformed_json_and_wrong_vector_lengths_are_rejected() {
    assert!(SceneData::from_json("{").is_err());
    for (pointer, field) in [
        ("", "schema_version"),
        ("/entities/0", "name"),
        ("/camera", "near"),
    ] {
        let mut value = as_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(SceneData::from_json(&value.to_string()).is_err());
    }
    for position in [json!([0.0, 0.0]), json!([0.0, 0.0, 0.0, 0.0])] {
        let mut value = as_value();
        value["entities"][0]["transform"]["position"] = position;
        assert!(SceneData::from_json(&value.to_string()).is_err());
    }
}

#[test]
fn blank_names_and_nonportable_or_parent_model_paths_fail_load_and_save_validation() {
    for name in ["", " ", "\t\n"] {
        let mut data = valid_data();
        data.entities[0].name = name.to_owned();
        assert_rejected(&data);
        assert!(SceneData::from_json(&serde_json::to_string(&data).unwrap()).is_err());
    }
    for path in [
        "",
        " \t",
        "/props/robot.glb",
        "C:/robot.glb",
        "C:robot.glb",
        "props\\robot.glb",
        "\\\\server\\robot.glb",
        "../robot.glb",
        "props/../robot.glb",
        "props/..",
        "https://host/robot.glb",
    ] {
        let mut data = valid_data();
        data.entities[0].model_path = path.to_owned();
        assert_rejected(&data);
        assert!(
            SceneData::from_json(&serde_json::to_string(&data).unwrap()).is_err(),
            "accepted {path:?}"
        );
    }
}

#[test]
fn every_nonfinite_transform_or_camera_component_is_rejected_before_serializing() {
    for nonfinite in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for field in 0..17 {
            let mut data = valid_data();
            let transform = &mut data.entities[1].transform;
            let camera = &mut data.camera;
            let value = match field {
                0..=2 => &mut transform.position[field],
                3..=5 => &mut transform.rotation[field - 3],
                6..=8 => &mut transform.scale[field - 6],
                9..=11 => &mut camera.position[field - 9],
                12 => &mut camera.yaw,
                13 => &mut camera.pitch,
                14 => &mut camera.vertical_fov,
                15 => &mut camera.near_plane,
                _ => &mut camera.far_plane,
            };
            *value = nonfinite;
            assert_rejected(&data);
        }
    }
}

#[test]
fn camera_boundaries_match_runtime_pitch_and_valid_projection_ranges() {
    let limit = 89.0_f32.to_radians();
    let pi = std::f32::consts::PI;
    for (pitch, fov, near, far, accepted) in [
        (-limit, 1.0, 0.1, 100.0, true),
        (limit, 1.0, 0.1, 100.0, true),
        (-limit - 0.001, 1.0, 0.1, 100.0, false),
        (limit + 0.001, 1.0, 0.1, 100.0, false),
        (0.0, 0.0001, 0.1, 100.0, true),
        (0.0, pi - 0.0001, 0.1, 100.0, true),
        (0.0, 0.0, 0.1, 100.0, false),
        (0.0, -0.1, 0.1, 100.0, false),
        (0.0, pi, 0.1, 100.0, false),
        (0.0, 1.0, 0.0001, 0.0002, true),
        (0.0, 1.0, 0.0, 100.0, false),
        (0.0, 1.0, -0.1, 100.0, false),
        (0.0, 1.0, 0.1, 0.1, false),
        (0.0, 1.0, 0.2, 0.1, false),
    ] {
        let mut data = valid_data();
        data.camera.pitch = pitch;
        data.camera.vertical_fov = fov;
        data.camera.near_plane = near;
        data.camera.far_plane = far;
        assert_eq!(
            data.validate().is_ok(),
            accepted,
            "camera boundary: {:?}",
            data.camera
        );
        if accepted {
            SceneData::from_json(&data.to_json().unwrap()).unwrap();
        } else {
            assert_rejected(&data);
        }
    }
}

#[test]
fn bundled_scene_remains_valid_and_keeps_demo_role_order() {
    let source = include_str!("../../../apps/game/assets/scenes/robots.json");
    let data = SceneData::from_json(source).unwrap();
    assert_eq!(data.entities.len(), 2);
    assert_eq!(data.entities[0].name, "Left Robot");
    assert_eq!(data.entities[1].name, "Right Robot");
    for entity in &data.entities {
        let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/game/assets")
            .join(&entity.model_path);
        assert!(
            model.is_file(),
            "missing bundled model: {}",
            model.display()
        );
    }
    let roundtrip = SceneData::from_json(&data.to_json().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(data).unwrap(),
        serde_json::to_value(roundtrip).unwrap()
    );
}

#[test]
fn empty_scenes_and_duplicate_unicode_labels_roundtrip_without_changing_order() {
    let mut data = valid_data();
    for entity in &mut data.entities {
        entity.name = "רובוט 🤖".to_owned();
    }
    data.entities[0].transform.scale = [-1.0, 0.0, 1.0];
    let parsed = SceneData::from_json(&data.to_json().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&data).unwrap(),
        serde_json::to_value(parsed).unwrap()
    );
    data.entities.clear();
    assert!(
        SceneData::from_json(&data.to_json().unwrap())
            .unwrap()
            .entities
            .is_empty()
    );
}
