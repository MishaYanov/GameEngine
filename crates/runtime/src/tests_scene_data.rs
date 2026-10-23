use super::*;
use serde_json::{Value, json};

fn valid_data() -> SceneData {
    let mut btm = BTreeMap::new();
    btm.insert("robot-0".to_owned(), EntityKey::new("robot-0").unwrap());
    btm.insert("robot-1".to_owned(), EntityKey::new("robot-1").unwrap());

    SceneData {
        schema_version: CURRENT_SCHEMA_VERSION,
        bindings: btm,
        entities: [-0.8, 0.8]
            .into_iter()
            .enumerate()
            .map(|(index, x)| EntityData {
                key: EntityKey::new(format!("robot-{index}")).unwrap(),
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
    for version in [0, 3, u32::MAX] {
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
fn bundled_scene_remains_valid_and_binds_demo_roles_to_persistent_keys() {
    let source = include_str!("../../../apps/game/assets/scenes/robots.json");
    let data = SceneData::from_json(source).unwrap();
    assert_eq!(data.entities.len(), 2);
    assert_eq!(data.entities[0].name, "Left Robot");
    assert_eq!(data.entities[1].name, "Right Robot");
    assert_eq!(data.bindings["robot_a"], data.entities[0].key);
    assert_eq!(data.bindings["robot_b"], data.entities[1].key);
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

fn legacy_value() -> Value {
    let mut value = as_value();
    value["schema_version"] = json!(1);
    value.as_object_mut().unwrap().remove("bindings");
    for entity in value["entities"].as_array_mut().unwrap() {
        entity.as_object_mut().unwrap().remove("key");
    }
    value
}

#[test]
fn legacy_json_migrates_deterministically_and_saves_as_version_two() {
    let legacy = legacy_value();
    let original = legacy.to_string();
    let parsed = parse_json(&original).unwrap();
    assert_eq!(parsed.source_schema_version, 1);
    assert_eq!(parsed.data.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(parsed.data.bindings.is_empty());
    for (index, entity) in parsed.data.entities.iter().enumerate() {
        assert_eq!(entity.key.as_str(), format!("legacy-entity-{index}"));
        assert_eq!(
            serde_json::to_value(&entity.transform).unwrap(),
            legacy["entities"][index]["transform"]
        );
        assert_eq!(
            entity.name,
            legacy["entities"][index]["name"].as_str().unwrap()
        );
    }
    let repeated = parse_json(&original).unwrap();
    let saved = parsed.data.to_json().unwrap();
    assert_eq!(saved, repeated.data.to_json().unwrap());
    let reloaded = parse_json(&saved).unwrap();
    assert_eq!(reloaded.source_schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(reloaded.data.to_json().unwrap(), saved);
}

#[test]
fn legacy_json_still_rejects_unknown_fields_including_version_two_metadata() {
    for (pointer, field, extra) in [
        ("", "bindings", json!({})),
        ("/entities/0", "key", json!("unexpected-key")),
        ("/entities/0/transform", "unexpected", json!(true)),
        ("/camera", "unexpected", json!(true)),
    ] {
        let mut legacy = legacy_value();
        legacy
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), extra);
        assert!(
            SceneData::from_json(&legacy.to_string())
                .unwrap_err()
                .contains("unknown field")
        );
    }
}

#[test]
fn current_schema_rejects_invalid_duplicate_keys_and_dangling_bindings() {
    for key in [
        "",
        " ",
        "has space",
        "tab\tkey",
        "line\nbreak",
        "nonbreaking\u{a0}space",
    ] {
        let mut value = as_value();
        value["entities"][0]["key"] = json!(key);
        assert!(
            SceneData::from_json(&value.to_string()).is_err(),
            "accepted {key:?}"
        );
    }
    let mut duplicate = valid_data();
    duplicate.entities[1].key = duplicate.entities[0].key.clone();
    assert_rejected(&duplicate);
    for (name, key) in [
        ("", "robot-0"),
        (" \t", "robot-0"),
        ("robot_a", "missing"),
        ("robot_a", "bad key"),
    ] {
        let mut value = as_value();
        value["bindings"] = json!({ name: key });
        assert!(
            SceneData::from_json(&value.to_string()).is_err(),
            "accepted binding {name:?}: {key:?}"
        );
    }
    for (pointer, field) in [("", "bindings"), ("/entities/0", "key")] {
        let mut value = as_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(SceneData::from_json(&value.to_string()).is_err());
    }
}

#[test]
fn entity_reordering_preserves_named_bindings_and_persistent_keys() {
    let mut data = valid_data();
    let bindings = data.bindings.clone();
    data.entities.reverse();
    let parsed = SceneData::from_json(&data.to_json().unwrap()).unwrap();
    assert_eq!(parsed.bindings, bindings);
    assert_eq!(parsed.entities[0].key.as_str(), "robot-1");
    assert_eq!(parsed.entities[1].key.as_str(), "robot-0");
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
    data.bindings.clear();
    assert!(
        SceneData::from_json(&data.to_json().unwrap())
            .unwrap()
            .entities
            .is_empty()
    );
}
