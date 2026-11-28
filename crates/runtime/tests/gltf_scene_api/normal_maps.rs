use std::io::Cursor;

use runtime::assets::gltf_loader::{self, ImportedNormalDetail};
use serde_json::{Value, json};

use super::{Directory, fs, write};

fn fixture() -> (Value, Vec<u8>, Vec<u8>) {
	let mut bytes = Vec::new();
	let mut views = Vec::new();
	let mut accessors = Vec::new();
	for (values, kind) in [
		(&[0., 0., 0., 1., 0., 0., 0., 1., 0.][..], "VEC3"),
		(&[0., 0., 1., 0., 0., 1., 0., 0., 1.][..], "VEC3"),
		(&[0., 0., 0., 1., 1., 0.][..], "VEC2"),
		(&[0., 0., 1., 0., 0., 1.][..], "VEC2"),
		(
			&[1., 0., 0., -1., 1., 0., 0., -1., 1., 0., 0., -1.][..],
			"VEC4",
		),
	] {
		let offset = bytes.len();
		for &value in values {
			bytes.extend_from_slice(&f32::to_le_bytes(value));
		}
		views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":values.len()*4}));
		accessors
			.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":3,"type":kind}));
	}
	accessors[0]["min"] = json!([0, 0, 0]);
	accessors[0]["max"] = json!([1, 1, 0]);
	let mut png = Cursor::new(Vec::new());
	image::RgbaImage::from_raw(1, 1, vec![204, 128, 230, 255])
		.unwrap()
		.write_to(&mut png, image::ImageFormat::Png)
		.unwrap();
	let mut model = json!({
		"asset":{"version":"2.0"},
		"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
		"buffers":[{"byteLength":bytes.len()}],"bufferViews":views,"accessors":accessors,
		"images":[{"uri":"direction.png"}],"textures":[{"source":0}],
		"materials":[{"normalTexture":{"index":0,"texCoord":1},
			"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],
		"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2,"TEXCOORD_1":3},"material":0}]}]
	});
	model["buffers"][0]["uri"] = json!("geometry.bin");
	(model, bytes, png.into_inner())
}

#[test]
fn external_normal_uv_is_independent_and_shares_its_decoded_image_with_color() {
	let directory = Directory::new("normal-maps-external");
	let (mut model, bytes, png) = fixture();
	fs::write(directory.0.join("geometry.bin"), bytes).unwrap();
	fs::write(directory.0.join("direction.png"), png).unwrap();
	let path = write(&directory, &model);
	let mesh = gltf_loader::load_first_mesh(&path).unwrap();
	let part = &mesh[0];
	let normal = part.normal_map.as_ref().unwrap();
	assert_eq!(normal.scale, 1.0);
	assert_eq!(normal.image_index, 0);
	assert_eq!(normal.image.pixels(), [204, 128, 230, 255]);
	assert!(std::ptr::eq(
		normal.image.pixels().as_ptr(),
		part.material
			.base_color_image
			.as_ref()
			.unwrap()
			.pixels()
			.as_ptr(),
	));
	assert_eq!(part.mesh.vertices()[1].uv, [0., 1.]);
	assert_eq!(part.mesh.vertices()[1].normal_uv, [1., 0.]);
	assert_eq!(part.mesh.vertices()[1].tangent, [1., 0., 0., 1.]);
	let metadata = gltf_loader::load_scene(&path).unwrap();
	assert_eq!(metadata.materials[0].normal_uv_set, Some(1));
	assert_eq!(
		metadata.meshes[0].primitives[0].normal_detail,
		ImportedNormalDetail::GeneratedTangents
	);

	model["materials"][0]["normalTexture"]["scale"] = json!(0.0);
	let path = write(&directory, &model);
	assert_eq!(
		gltf_loader::load_first_mesh(&path).unwrap()[0]
			.normal_map
			.as_ref()
			.unwrap()
			.scale,
		0.0
	);
	assert_eq!(
		gltf_loader::load_scene(&path).unwrap().meshes[0].primitives[0].normal_detail,
		ImportedNormalDetail::StrengthZero
	);

	for (uv, scale) in [(2, 1.), (1, -1.)] {
		model["materials"][0]["normalTexture"] = json!({"index":0,"texCoord":uv,"scale":scale});
		let Err(error) = gltf_loader::load_first_mesh(write(&directory, &model)) else {
			panic!("invalid normal binding was accepted");
		};
		assert!(error.to_string().contains("mesh 0, primitive 0"), "{error}");
	}
	model["materials"][0]["normalTexture"] = json!({"index":99, "texCoord":1});
	assert!(gltf_loader::load_first_mesh(write(&directory, &model)).is_err());
	model["materials"][0]
		.as_object_mut()
		.unwrap()
		.remove("normalTexture");
	assert!(
		gltf_loader::load_first_mesh(write(&directory, &model)).unwrap()[0]
			.normal_map
			.is_none()
	);
}

#[test]
fn embedded_normal_keeps_authored_tangent_handedness() {
	let directory = Directory::new("normal-maps-embedded");
	let (mut model, mut bytes, png) = fixture();
	let view = model["bufferViews"].as_array_mut().unwrap();
	let image_view = view.len();
	view.push(json!({"buffer":0,"byteOffset":bytes.len(),"byteLength":png.len()}));
	bytes.extend_from_slice(&png);
	model["buffers"] = json!([{"byteLength":bytes.len()}]);
	model["images"] = json!([{"bufferView":image_view,"mimeType":"image/png"}]);
	model["meshes"][0]["primitives"][0]["attributes"]["TANGENT"] = json!(4);
	let mut json = model.to_string().into_bytes();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	while !bytes.len().is_multiple_of(4) {
		bytes.push(0);
	}
	let glb = gltf::binary::Glb {
		header: gltf::binary::Header {
			magic: *b"glTF",
			version: 2,
			length: (12 + 16 + json.len() + bytes.len()) as u32,
		},
		json: json.into(),
		bin: Some(bytes.into()),
	};
	let path = directory.0.join("normal.glb");
	fs::write(&path, glb.to_vec().unwrap()).unwrap();
	let mesh = gltf_loader::load_first_mesh(&path).unwrap();
	assert_eq!(mesh[0].mesh.vertices()[1].tangent, [1., 0., 0., -1.]);
	assert_eq!(
		gltf_loader::load_scene(&path).unwrap().meshes[0].primitives[0].normal_detail,
		ImportedNormalDetail::AuthoredTangents
	);
}
