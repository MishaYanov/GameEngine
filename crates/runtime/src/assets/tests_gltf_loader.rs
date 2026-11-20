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
fn mesh_preflight_counts_unique_meshes_and_rejects_aggregate_decoded_storage_without_allocating_it()
{
	let document = gltf::Gltf::from_slice(MODEL.as_bytes()).unwrap().document;
	let one = preflight_meshes(&document, [0]).unwrap();
	assert!(one > 0);
	assert_eq!(preflight_meshes(&document, [0, 0, 0]).unwrap(), one);
	assert!(preflight_meshes(&document, [1]).is_err());

	// Accessor metadata describes the oversized work; no decoded arrays are allocated.
	let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
	value["accessors"][0]["count"] = serde_json::json!(1_000_000);
	let mesh = value["meshes"][0].clone();
	value["meshes"] = serde_json::json!([mesh.clone(), mesh]);
	let document = gltf::Gltf::from_slice(value.to_string().as_bytes())
		.unwrap()
		.document;
	assert!(preflight_meshes(&document, [0]).is_ok());
	assert!(
		preflight_meshes(&document, [0, 1])
			.unwrap_err()
			.to_string()
			.contains("256 MiB")
	);
}

#[test]
fn mesh_preflight_validates_normal_and_uv_counts_before_decoding() {
	let (document, _) = normal_fixture([0.0, 0.0, 1.0], 2);
	assert!(
		preflight_meshes(&document, [0])
			.unwrap_err()
			.to_string()
			.contains("NORMAL")
	);
	let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
	value["accessors"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({"bufferView":0,"componentType":5126,"count":2,"type":"VEC2"}));
	value["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = serde_json::json!(2);
	let document = gltf::Gltf::from_slice(value.to_string().as_bytes())
		.unwrap()
		.document;
	assert!(matches!(
		preflight_meshes(&document, [0]),
		Err(GltfLoaderError::TexCoordCountMismatch {
			positions: 3,
			tex_coords: 2
		})
	));
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
fn shared_images_are_converted_once_and_survive_other_primitives() {
	let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
	value["images"] = serde_json::json!([{"uri":"shared.png"}]);
	value["textures"] = serde_json::json!([{"source":0}]);
	value["buffers"][0]["byteLength"] = serde_json::json!(68);
	value["bufferViews"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({"buffer":0,"byteOffset":44,"byteLength":24}));
	value["accessors"]
		.as_array_mut()
		.unwrap()
		.push(serde_json::json!({"bufferView":2,"componentType":5126,"count":3,"type":"VEC2"}));
	for primitive in value["meshes"][0]["primitives"].as_array_mut().unwrap() {
		primitive["attributes"]["TEXCOORD_0"] = serde_json::json!(2);
	}
	for material in value["materials"].as_array_mut().unwrap() {
		material["pbrMetallicRoughness"]["baseColorTexture"] = serde_json::json!({"index":0});
	}
	let document = gltf::Gltf::from_slice(value.to_string().as_bytes())
		.unwrap()
		.document;
	let mut buffers = geometry();
	buffers[0].0.resize(68, 0);
	for (color, pixels) in [
		(image::ColorType::Rgb8, vec![1, 2, 3]),
		(image::ColorType::Rgba8, vec![1, 2, 3, 255]),
	] {
		let mut encoded = Vec::new();
		image::ImageEncoder::write_image(
			image::codecs::png::PngEncoder::new(&mut encoded),
			&pixels,
			1,
			1,
			color.into(),
		)
		.unwrap();
		let mut budget = IMPORT_LIMIT;
		let images = [ImageResource::Decoded(
			decode_image(&encoded, &mut budget).unwrap(),
		)];
		let mut parts = import_first_mesh(&document, &buffers, &images).unwrap();
		let first = parts[0].material.base_color_image.as_ref().unwrap();
		let second = parts[1].material.base_color_image.as_ref().unwrap();
		assert_eq!(first.pixels().as_ptr(), second.pixels().as_ptr());
		assert_ne!(
			parts[0].material.base_color_factor,
			parts[1].material.base_color_factor
		);
		parts.remove(0);
		assert_eq!(
			parts[0]
				.material
				.base_color_image
				.as_ref()
				.unwrap()
				.pixels(),
			&[1, 2, 3, 255]
		);
	}
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

struct ImageFixture {
	root: std::path::PathBuf,
	path: std::path::PathBuf,
}

impl ImageFixture {
	fn new() -> Self {
		let root = std::env::temp_dir().join(format!("gltf-images-{}", uuid::Uuid::new_v4()));
		std::fs::create_dir(&root).unwrap();
		let path = root.join("model.gltf");
		let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
		let mut bytes = geometry().remove(0).0;
		bytes.resize(68, 0);
		value["buffers"][0] = serde_json::json!({
			"byteLength": bytes.len(),
			"uri": format!("data:application/octet-stream;base64,{}", base64::encode(bytes)),
		});
		value["bufferViews"]
			.as_array_mut()
			.unwrap()
			.push(serde_json::json!({"buffer":0,"byteOffset":44,"byteLength":24}));
		value["accessors"]
			.as_array_mut()
			.unwrap()
			.push(serde_json::json!({"bufferView":2,"componentType":5126,"count":3,"type":"VEC2"}));
		value["images"] = serde_json::json!([
			{"uri":"unused.bin"}, {"uri":"shared.png"}, {"uri":"later.png"},
		]);
		value["textures"] = serde_json::json!([{"source":1}, {"source":2}]);
		for material in value["materials"].as_array_mut().unwrap() {
			material["pbrMetallicRoughness"]["baseColorTexture"] = serde_json::json!({"index":0});
		}
		for primitive in value["meshes"][0]["primitives"].as_array_mut().unwrap() {
			primitive["attributes"]["TEXCOORD_0"] = serde_json::json!(2);
		}
		let mesh = value["meshes"][0].clone();
		value["meshes"].as_array_mut().unwrap().push(mesh.clone());
		value["meshes"].as_array_mut().unwrap().push(mesh);
		value["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"] =
			serde_json::json!({"index":1});
		// Meshes 0 and 1 share image 1; mesh 2 needs images 1 and 2.
		for mesh in 0..2 {
			value["meshes"][mesh]["primitives"][1]["material"] = serde_json::json!(0);
		}
		std::fs::write(&path, value.to_string()).unwrap();
		std::fs::write(root.join("unused.bin"), b"not an image").unwrap();
		image::RgbaImage::from_raw(1, 1, vec![1, 2, 3, 255])
			.unwrap()
			.save(root.join("shared.png"))
			.unwrap();
		image::RgbaImage::from_raw(1, 1, vec![4, 5, 6, 255])
			.unwrap()
			.save(root.join("later.png"))
			.unwrap();
		Self { root, path }
	}
}

impl Drop for ImageFixture {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.root);
	}
}

#[test]
fn selected_mesh_images_keep_indices_share_pixels_and_preserve_the_source_snapshot() {
	let fixture = ImageFixture::new();
	let mut resources = load_resources(&fixture.path).unwrap();
	assert!(
		resources
			.images
			.iter()
			.all(|image| matches!(image, ImageResource::Encoded(_)))
	);
	// Later mesh requests must still use the same bounded source snapshot.
	std::fs::remove_file(fixture.root.join("shared.png")).unwrap();
	std::fs::remove_file(fixture.root.join("later.png")).unwrap();
	resources.prepare_mesh_images(0).unwrap();
	assert!(matches!(resources.images[0], ImageResource::Encoded(_)));
	assert!(matches!(resources.images[1], ImageResource::Decoded(_)));
	assert!(matches!(resources.images[2], ImageResource::Encoded(_)));
	let first = resources.import_mesh(0).unwrap();
	let address = first[0]
		.material
		.base_color_image
		.as_ref()
		.unwrap()
		.pixels()
		.as_ptr();
	let budget = resources.budget;
	resources.prepare_mesh_images(1).unwrap();
	assert_eq!(
		resources.budget, budget,
		"shared images must not be charged again"
	);
	let second = resources.import_mesh(1).unwrap();
	assert_eq!(second[0].base_color_image_index, Some(1));
	assert_eq!(
		second[0]
			.material
			.base_color_image
			.as_ref()
			.unwrap()
			.pixels()
			.as_ptr(),
		address
	);
	resources.prepare_mesh_images(2).unwrap();
	let third = resources.import_mesh(2).unwrap();
	assert_eq!(third[1].base_color_image_index, Some(2));
	assert_eq!(
		third[1]
			.material
			.base_color_image
			.as_ref()
			.unwrap()
			.pixels(),
		&[4, 5, 6, 255]
	);
	drop(resources);
	drop(first);
	assert_eq!(
		second[0]
			.material
			.base_color_image
			.as_ref()
			.unwrap()
			.pixels()
			.as_ptr(),
		address
	);
}

#[test]
fn image_preparation_failure_preserves_cached_pixels_budget_and_retained_accounting() {
	let fixture = ImageFixture::new();
	std::fs::write(fixture.root.join("later.png"), b"invalid image").unwrap();
	let mut resources = load_resources(&fixture.path).unwrap();
	let bytes = resources.retained_bytes();
	let budget = resources.budget;
	assert!(resources.prepare_mesh_images(2).is_err());
	assert!(
		resources
			.images
			.iter()
			.all(|image| matches!(image, ImageResource::Encoded(_)))
	);
	assert_eq!(resources.budget, budget);
	assert_eq!(resources.retained_bytes(), bytes);
	resources.prepare_mesh_images(0).unwrap();
	let ImageResource::Decoded(image) = &resources.images[1] else {
		panic!("image was not decoded")
	};
	let address = image.pixels().as_ptr();
	let budget = resources.budget;
	let bytes = resources.retained_bytes();
	assert!(resources.prepare_mesh_images(2).is_err());
	assert_eq!(resources.budget, budget);
	assert_eq!(resources.retained_bytes(), bytes);
	let ImageResource::Decoded(image) = &resources.images[1] else {
		panic!("cached image was lost")
	};
	assert_eq!(image.pixels().as_ptr(), address);
}

#[test]
fn deferred_image_decode_enforces_remaining_budget_before_publishing() {
	let fixture = ImageFixture::new();
	let mut resources = load_resources(&fixture.path).unwrap();
	resources.budget = 3;
	let bytes = resources.retained_bytes();
	assert!(resources.prepare_mesh_images(0).is_err());
	assert_eq!(resources.budget, 3);
	assert_eq!(resources.retained_bytes(), bytes);
	assert!(matches!(resources.images[1], ImageResource::Encoded(_)));
}

#[test]
fn public_first_mesh_ignores_unused_image_pixels_but_validates_their_references() {
	let fixture = ImageFixture::new();
	assert_eq!(load_first_mesh(&fixture.path).unwrap().len(), 2);
	let mut value: serde_json::Value =
		serde_json::from_slice(&std::fs::read(&fixture.path).unwrap()).unwrap();
	value["images"][0]["uri"] = serde_json::json!("file:///outside.png");
	std::fs::write(&fixture.path, value.to_string()).unwrap();
	assert!(matches!(
		load_first_mesh(&fixture.path),
		Err(GltfLoaderError::Resource(_))
	));
}
