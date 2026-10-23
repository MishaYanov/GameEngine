use super::*;

fn read_uri(root: &Path, uri: &str, budget: &mut usize) -> Result<Vec<u8>, GltfLoaderError> {
    read_uri_in_root(root, root, uri, budget)
}

fn normal_fixture(normal: [f32; 3], count: usize) -> (gltf::Document, Vec<gltf::buffer::Data>) {
	let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
	value["buffers"][0]["byteLength"] = serde_json::json!(80);
	value["bufferViews"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({"buffer":0,"byteOffset":44,"byteLength":36}));
	value["accessors"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({"bufferView":2,"componentType":5126,"count":count,"type":"VEC3"}));
	for primitive in value["meshes"][0]["primitives"].as_array_mut().unwrap() {
		primitive["attributes"]["NORMAL"] = serde_json::json!(2);
	}
	let mut buffers = geometry();
	buffers[0].0.extend_from_slice(&[0, 0]);
	for _ in 0..3 {
		for value in normal {
			buffers[0].0.extend_from_slice(&value.to_le_bytes());
		}
	}
	(
		gltf::Gltf::from_slice(value.to_string().as_bytes())
			.unwrap()
			.document,
		buffers,
	)
}

#[test]
fn gltf_import_preserves_provided_normal_direction_and_normalizes_it() {
	let (document, buffers) = normal_fixture([2.0, 0.0, 0.0], 3);
	let parts = import_first_mesh(&document, &buffers, &[]).unwrap();
	for part in parts {
		for vertex in part.mesh.vertices() {
			assert_eq!(vertex.normal, [1.0, 0.0, 0.0]);
		}
	}
}

#[test]
fn gltf_import_rejects_zero_nonfinite_or_mismatched_normals() {
	for (normal, count) in [
		([0.0; 3], 3),
		([f32::NAN, 0.0, 1.0], 3),
		([0.0, 0.0, 1.0], 2),
	] {
		let (document, buffers) = normal_fixture(normal, count);
		assert!(import_first_mesh(&document, &buffers, &[]).is_err());
	}
}

const MODEL: &str = r#"{
    "asset":{"version":"2.0"},
    "buffers":[{"byteLength":42}],
    "bufferViews":[
        {"buffer":0,"byteOffset":0,"byteLength":36},
        {"buffer":0,"byteOffset":36,"byteLength":6}
    ],
    "accessors":[
        {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3",
         "min":[0,0,0],"max":[1,1,0]},
        {"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}
    ],
    "materials":[
        {"doubleSided":true,"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}},
        {"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,1]}}
    ],
    "meshes":[{"primitives":[
        {"attributes":{"POSITION":0},"indices":1,"material":0,"mode":4},
        {"attributes":{"POSITION":0},"indices":1,"material":1,"mode":4}
    ]}]
}"#;

fn geometry() -> Vec<gltf::buffer::Data> {
    let mut bytes = Vec::new();
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for index in [0_u16, 1, 2] {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    vec![gltf::buffer::Data(bytes)]
}

#[test]
fn imports_all_parts_with_their_own_materials() {
    let document = gltf::Gltf::from_slice(MODEL.as_bytes()).unwrap().document;
    let parts = import_first_mesh(&document, &geometry(), &[]).unwrap();
    assert_eq!(parts.len(), 2);
    for (index, part) in parts.iter().enumerate() {
        assert_eq!(part.mesh_index, 0);
        assert_eq!(part.primitive_index, index);
        assert_eq!(part.mesh.vertices().len(), 3);
        assert_eq!(part.mesh.indices(), &[0, 1, 2]);
        assert_eq!(part.base_color_image_index, None);
        assert!(part.material.base_color_image.is_none());
    }
    assert!(parts[0].material.double_sided);
    assert!(!parts[1].material.double_sided);
    assert_eq!(parts[0].material.base_color_factor, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(parts[1].material.base_color_factor, [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn rejects_unsupported_parts_instead_of_silently_skipping_them() {
    let model = MODEL.replacen("\"mode\":4", "\"mode\":1", 1);
    let document = gltf::Gltf::from_slice(model.as_bytes()).unwrap().document;
    assert!(matches!(
        import_first_mesh(&document, &geometry(), &[]),
        Err(GltfLoaderError::UnsupportedMode(Mode::Lines))
    ));
}

#[test]
fn rejects_indices_outside_the_vertex_buffer() {
    let document = gltf::Gltf::from_slice(MODEL.as_bytes()).unwrap().document;
    let mut buffers = geometry();
    buffers[0].0[40..42].copy_from_slice(&3_u16.to_le_bytes());
    assert!(matches!(
        import_first_mesh(&document, &buffers, &[]),
        Err(GltfLoaderError::IndexOutOfBounds {
            index: 3,
            vertex_count: 3
        })
    ));
}

#[test]
fn factor_only_materials_get_safe_default_uvs() {
    let document = gltf::Gltf::from_slice(MODEL.as_bytes()).unwrap().document;
    let parts = import_first_mesh(&document, &geometry(), &[]).unwrap();
    for part in parts {
        for vertex in part.mesh.vertices() {
            assert_eq!(vertex.uv, [0.0, 0.0]);
        }
    }
}

#[test]
fn external_references_cannot_escape_or_use_schemes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    for uri in [
        "../Cargo.toml",
        "%2e%2e/Cargo.toml",
        "file:///etc/passwd",
        "https://example.com/model.bin",
        "C:/secret.bin",
        "\\\\server/share",
        "/etc/passwd",
        "%FF",
        "a%00b",
    ] {
        let mut budget = IMPORT_LIMIT;
        assert!(read_uri(&root, uri, &mut budget).is_err(), "accepted {uri}");
    }
    let mut budget = IMPORT_LIMIT;
    let bytes = read_uri(&root, "Cargo.toml", &mut budget).unwrap();
    assert_eq!(bytes, std::fs::read(root.join("Cargo.toml")).unwrap());
    let mut budget = IMPORT_LIMIT;
    assert_eq!(
        read_uri_in_root(&root.join("src"), &root, "../Cargo.toml", &mut budget).unwrap(),
        bytes
    );
}

#[test]
fn resources_enforce_remaining_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    assert!(read_uri(&root, "Cargo.toml", &mut 1).is_err());
    assert!(
        read_uri(
            &root,
            "data:application/octet-stream;base64,AQIDBA==",
            &mut 3
        )
        .is_err()
    );
    assert_eq!(
        read_uri(
            &root,
            "data:application/octet-stream;base64,AQIDBA==",
            &mut 4
        )
        .unwrap(),
        [1, 2, 3, 4]
    );
}

#[test]
fn bundled_glb_still_loads() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/game/assets/props/robot.glb");
    assert!(!load_first_mesh(path).unwrap().is_empty());
}

#[test]
fn rejects_escaping_buffer_and_image_before_import() {
    let root = std::env::temp_dir().join(format!("gltf-security-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    {
        let model = root.join("model.gltf");
        for json in [
            MODEL.replace("\"byteLength\":42", "\"byteLength\":42,\"uri\":\"../outside.bin\""),
            MODEL.replace("\"byteLength\":42", "\"byteLength\":42,\"uri\":\"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"")
                .replace("\"materials\":[", "\"images\":[{\"uri\":\"file:///outside.png\"}],\"materials\":["),
        ] {
            std::fs::write(&model, json).unwrap();
            assert!(matches!(load_resources(&model), Err(GltfLoaderError::Resource(_))));
        }
        let large = root.join("large.bin");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(RESOURCE_LIMIT as u64 + 1)
            .unwrap();
        let mut budget = IMPORT_LIMIT;
        assert!(read_bounded(&large, &mut budget).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}
