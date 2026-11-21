use std::sync::Arc;

use bridge::animation_asset;
use engine_core::{
	animation::{
		Interpolation, checked_similarity_inverse, clip_time, global_pose, mesh_local_palette,
		sample_local, sample_pose, short_slerp,
	},
	asset_catalog::AssetId,
};
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};

use super::{GLB, close, decoded, key, root, scene};

#[test]
fn fixture_decodes_three_clips_and_shared_source_instances_share_one_immutable_asset() {
	let asset = decoded();
	assert_eq!(asset.skin.joints.len(), 2);
	assert_eq!(asset.nodes.len(), 4);
	assert_eq!(asset.primitives.len(), 2);
	assert_eq!(
		asset
			.clips
			.iter()
			.map(|c| c.name.as_str())
			.collect::<Vec<_>>(),
		["Idle", "Walk", "Air"]
	);
	let mut data = scene(true);
	let mut second = data.entities[0].clone();
	second.key = key("copy");
	data.entities.push(second);
	let bindings = animation_asset::prepare_scene(&root(), &data).unwrap();
	assert!(Arc::ptr_eq(
		&bindings[&key("clip-demo")],
		&bindings[&key("copy")]
	));
	let id = AssetId::new("a9a07900-9dc2-4d9f-8ff1-718defec0291").unwrap();
	let metadata = animation_asset::inspect(&root(), &id).unwrap();
	assert_eq!(metadata.source.mesh_node, 3);
	assert_eq!(metadata.clips.len(), 3);
	let mut wrong = metadata.source;
	wrong.expected_sha256 = "0".repeat(64);
	assert!(
		animation_asset::load(&root(), &wrong)
			.unwrap_err()
			.contains("changed")
	);
	data.entities[1]
		.animation
		.as_mut()
		.unwrap()
		.settings
		.source
		.mesh_node = 2;
	assert!(
		animation_asset::prepare_scene(&root(), &data).is_err(),
		"shared cache must still validate each source binding"
	);
}

#[test]
fn decoder_rejects_bad_weights_cubic_sparse_and_nonuniform_basis() {
	let bad_weights = edit_glb(|doc, bin| {
		let accessor = doc["meshes"][0]["primitives"][0]["attributes"]["WEIGHTS_0"]
			.as_u64()
			.unwrap() as usize;
		let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
		let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
		bin[offset..offset + 16].fill(0);
	});
	assert!(
		animation_asset::decode(&bad_weights)
			.unwrap_err()
			.contains("weights")
	);
	let cubic = edit_glb(|doc, _| {
		doc["animations"][0]["samplers"][0]["interpolation"] = json!("CUBICSPLINE")
	});
	assert!(
		animation_asset::decode(&cubic)
			.unwrap_err()
			.contains("CUBICSPLINE")
	);
	let sparse = edit_glb(
		|doc, _| doc["accessors"][0]["sparse"] = json!({"count":1,"indices":{"bufferView":4,"componentType":5123},"values":{"bufferView":0}}),
	);
	assert!(animation_asset::decode(&sparse).is_err());
	let scale = edit_glb(|doc, _| doc["nodes"][0]["scale"] = json!([1, 2, 1]));
	assert!(
		animation_asset::decode(&scale)
			.unwrap_err()
			.contains("Nonuniform")
	);
}

#[test]
fn rest_palette_uses_mesh_local_basis_and_animated_envelope_contains_skinned_vertices() {
	let asset = decoded();
	let rest = asset.nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
	let globals = global_pose(&asset, &rest).unwrap();
	let palette = mesh_local_palette(&asset, &globals).unwrap();
	for matrix in palette {
		for (a, b) in matrix
			.to_cols_array()
			.iter()
			.zip(Mat4::IDENTITY.to_cols_array())
		{
			close(*a, b);
		}
	}
	close(globals[usize::from(asset.skin.mesh_node)].w_axis.x, 0.15);
	for scale in [0.01, 1., 8.] {
		let placement = Mat4::from_scale_rotation_translation(
			Vec3::splat(scale),
			Quat::from_rotation_y(0.4),
			Vec3::new(3., 2., -1.),
		);
		let pose = sample_pose(asset.clone(), 1, 0.5, placement, 7).unwrap();
		assert_eq!(pose.revision, 7);
		let min = pose.bounds.min();
		let max = pose.bounds.max();
		for vertex in asset.primitives.iter().flat_map(|p| &p.vertices) {
			let p = Vec3::from_array(vertex.position);
			let mut skin = Vec3::ZERO;
			for n in 0..4 {
				skin += pose.palette[usize::from(vertex.joints[n])].transform_point3(p)
					* vertex.weights[n];
			}
			let world = (placement * pose.mesh_matrix)
				.transform_point3(skin)
				.to_array();
			for i in 0..3 {
				assert!(
					world[i] >= min[i] && world[i] <= max[i],
					"vertex {world:?} outside {min:?}..{max:?}"
				);
			}
		}
	}
	assert!(checked_similarity_inverse(Mat4::from_scale(Vec3::new(-1., 1., 1.))).is_err());
	let mut shear = Mat4::IDENTITY;
	shear.y_axis.x = 0.3;
	assert!(checked_similarity_inverse(shear).is_err());
}

#[test]
fn sampler_obeys_step_linear_antipodal_quaternions_and_exact_loop_boundary() {
	let mut asset = (*decoded()).clone();
	let start = sample_local(&asset, 1, 0.).unwrap();
	let end = sample_local(&asset, 1, 0.5).unwrap();
	let middle = sample_local(&asset, 1, 0.25).unwrap();
	let node = usize::from(asset.clips[1].tracks[0].node);
	let rotated = Quat::from_array(middle[node].rotation_xyzw) * Vec3::Y;
	close(rotated.x, 0.);
	close(rotated.y, 1.);
	asset.clips[1].tracks[0].interpolation = Interpolation::Step;
	assert_eq!(sample_local(&asset, 1, 0.25).unwrap()[node], start[node]);
	assert_eq!(sample_local(&asset, 1, 0.5).unwrap()[node], end[node]);
	let q = Quat::from_rotation_z(0.7);
	let interpolated = Quat::from_array(short_slerp(q.to_array(), (-q).to_array(), 0.5).unwrap());
	assert!((interpolated * Vec3::X - q * Vec3::X).length() < 1e-5);
	assert_eq!(clip_time(1., 1., true).unwrap(), 0.);
	assert_eq!(clip_time(2., 1., false).unwrap(), 1.);
	assert_eq!(clip_time(8., 0., true).unwrap(), 0.);
	assert!(clip_time(f64::NAN, 1., true).is_err());
}

#[test]
fn embedded_base_color_images_are_decoded_once_and_sampler_subset_is_explicit() {
	let image = image::RgbaImage::from_raw(2, 1, vec![255, 20, 10, 255, 10, 200, 30, 255]).unwrap();
	let mut encoded = std::io::Cursor::new(Vec::new());
	image::DynamicImage::ImageRgba8(image)
		.write_to(&mut encoded, image::ImageFormat::Png)
		.unwrap();
	let encoded = encoded.into_inner();
	let textured = |unsupported_sampler: bool| {
		edit_glb(|doc, bin| {
			let view = doc["bufferViews"].as_array().unwrap().len();
			let offset = bin.len();
			bin.extend(&encoded);
			doc["bufferViews"]
				.as_array_mut()
				.unwrap()
				.push(json!({"buffer":0,"byteOffset":offset,"byteLength":encoded.len()}));
			doc["buffers"][0]["byteLength"] = json!(bin.len());
			doc["images"] = json!([{"bufferView":view,"mimeType":"image/png"}]);
			doc["textures"] = json!([{"source":0,"sampler":0}]);
			doc["samplers"] = json!([{"magFilter":9729,"minFilter":9729,"wrapS":if unsupported_sampler{33071}else{10497},"wrapT":10497}]);
			for material in doc["materials"].as_array_mut().unwrap() {
				material["pbrMetallicRoughness"]["baseColorTexture"] =
					json!({"index":0,"texCoord":0});
			}
		})
	};
	let asset = animation_asset::decode(&textured(false)).unwrap();
	assert_eq!(asset.images.len(), 1);
	assert_eq!((asset.images[0].width, asset.images[0].height), (2, 1));
	assert_eq!(asset.images[0].rgba, [255, 20, 10, 255, 10, 200, 30, 255]);
	assert!(
		asset
			.primitives
			.iter()
			.all(|p| p.base_color_image == Some(0))
	);
	assert_ne!(asset.primitives[0].color, asset.primitives[1].color);
	assert!(
		animation_asset::decode(&textured(true))
			.unwrap_err()
			.contains("linear/repeat")
	);
}

fn edit_glb(edit: impl FnOnce(&mut Value, &mut Vec<u8>)) -> Vec<u8> {
	let length = u32::from_le_bytes(GLB[12..16].try_into().unwrap()) as usize;
	let mut doc: Value = serde_json::from_slice(&GLB[20..20 + length]).unwrap();
	let mut bin = GLB[28 + length..].to_vec();
	edit(&mut doc, &mut bin);
	let mut json = serde_json::to_vec(&doc).unwrap();
	while !json.len().is_multiple_of(4) {
		json.push(b' ');
	}
	while !bin.len().is_multiple_of(4) {
		bin.push(0);
	}
	let total = 12 + 8 + json.len() + 8 + bin.len();
	let mut out = Vec::new();
	for word in [0x46546c67, 2, total as u32, json.len() as u32, 0x4e4f534a] {
		out.extend(word.to_le_bytes());
	}
	out.extend(json);
	out.extend((bin.len() as u32).to_le_bytes());
	out.extend(0x004e4942u32.to_le_bytes());
	out.extend(bin);
	out
}
