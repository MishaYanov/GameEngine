use std::sync::Arc;

use bridge::animation_asset;
use engine_core::{
	animation::{
		Interpolation, checked_similarity_inverse, clip_time, global_pose, mesh_local_palette,
		sample_local, sample_pose, short_slerp,
	},
	asset_catalog::{AssetCatalog, AssetId, CATALOG_FILE},
	project::ProjectData,
	simulation_session::SimulationSession,
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
	assert_eq!(metadata.joint_metadata.len(), 2);
	assert!(
		metadata
			.joint_metadata
			.iter()
			.all(|joint| joint.stable_id.is_none())
	);
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
fn skin_normal_map_keeps_generated_frames_and_preserves_skin_influences() {
	let bytes = edit_glb(|doc, bin| {
		let mut png = std::io::Cursor::new(Vec::new());
		image::RgbaImage::from_pixel(1, 1, image::Rgba([204, 128, 230, 255]))
			.write_to(&mut png, image::ImageFormat::Png)
			.unwrap();
		let png = png.into_inner();
		let views = doc["bufferViews"].as_array_mut().unwrap();
		let view = views.len();
		views.push(json!({"buffer":0,"byteOffset":bin.len(),"byteLength":png.len()}));
		bin.extend_from_slice(&png);
		doc["buffers"][0]["byteLength"] = json!(bin.len());
		doc["images"] = json!([{"bufferView":view,"mimeType":"image/png"}]);
		doc["textures"] = json!([{"source":0}]);
		doc["materials"][0]["normalTexture"] = json!({"index":0,"scale":0.65});
		doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index":0});
	});
	let reference = decoded();
	let asset = animation_asset::decode(&bytes).unwrap();
	let map = asset.primitives[0].normal_map.unwrap();
	assert_eq!(map.scale, 0.65);
	assert_eq!(asset.images.len(), 1);
	assert_eq!(asset.primitives[0].base_color_image, Some(map.image));
	assert!(asset.primitives[1].normal_map.is_none());
	assert_eq!(asset.images[map.image].rgba, [204, 128, 230, 255]);
	let normal_vertices = &asset.primitives[0].vertices;
	assert!(
		normal_vertices
			.iter()
			.any(|vertex| vertex.tangent[3].abs() == 1.0)
	);
	for vertex in normal_vertices {
		assert!(
			reference.primitives[0]
				.vertices
				.iter()
				.any(|source| vertex.position == source.position
					&& vertex.normal == source.normal
					&& vertex.joints == source.joints
					&& vertex.weights == source.weights
					&& vertex.normal_uv == source.uv)
		);
	}
	assert_eq!(asset.envelopes, reference.envelopes);
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

#[test]
fn character_reimport_preserves_clip_and_joint_ids_across_renames_and_reordering() {
	let original = edit_glb(|doc, _| mark_character(doc));
	let replacement = edit_glb(|doc, _| {
		mark_character(doc);
		let clips = doc["animations"].as_array_mut().unwrap();
		clips.swap(0, 1);
		clips[0]["name"] = json!("Renamed walk");
		let joints = doc["skins"][0]["joints"].as_array().unwrap();
		let first = joints[0].as_u64().unwrap();
		let second = joints[1].as_u64().unwrap();
		let remap = |node: &mut Value| {
			let old = node.as_u64().unwrap();
			*node = json!(if old == first {
				second
			} else if old == second {
				first
			} else {
				old
			});
		};
		let nodes = doc["nodes"].as_array_mut().unwrap();
		nodes.swap(first as usize, second as usize);
		nodes[second as usize]["name"] = json!("Renamed joint");
		for node in nodes {
			if let Some(children) = node["children"].as_array_mut() {
				children.iter_mut().for_each(remap);
			}
		}
		for scene in doc["scenes"].as_array_mut().unwrap() {
			scene["nodes"]
				.as_array_mut()
				.unwrap()
				.iter_mut()
				.for_each(remap);
		}
		let skin = &mut doc["skins"][0];
		skin["joints"]
			.as_array_mut()
			.unwrap()
			.iter_mut()
			.for_each(remap);
		if let Some(skeleton) = skin.get_mut("skeleton") {
			remap(skeleton);
		}
		for clip in doc["animations"].as_array_mut().unwrap() {
			for channel in clip["channels"].as_array_mut().unwrap() {
				remap(&mut channel["target"]["node"]);
			}
		}
	});
	let original_asset = animation_asset::decode(&original).unwrap();
	let replacement_asset = animation_asset::decode(&replacement).unwrap();
	replacement_asset
		.require_compatible_character(&original_asset)
		.unwrap();
	let id = AssetId::new("a9a07900-9dc2-4d9f-8ff1-718defec0291").unwrap();
	let source = original_asset.source_ref(id.clone());
	let walk = replacement_asset.resolve_clip_index(&source, 1).unwrap();
	assert_eq!(walk, 0);
	assert_eq!(replacement_asset.clips[walk as usize].name, "Renamed walk");
	assert_eq!(
		sample_local(&original_asset, 1, 0.2).unwrap(),
		sample_local(&replacement_asset, walk as usize, 0.2).unwrap(),
	);
	let previous_joint = original_asset.joint_node("joint-0").unwrap();
	let replacement_joint = replacement_asset.joint_node("joint-0").unwrap();
	assert_ne!(
		original_asset.nodes[usize::from(previous_joint)].source_node,
		replacement_asset.nodes[usize::from(replacement_joint)].source_node
	);

	let dir =
		std::env::temp_dir().join(format!("engine-character-{}", AssetId::generate().as_str()));
	let _cleanup = TestDirectory(&dir);
	let model_dir = dir.join("characters");
	std::fs::create_dir_all(&model_dir).unwrap();
	std::fs::write(model_dir.join("two-joint.glb"), &replacement).unwrap();
	let original_catalog = std::fs::read_to_string(root().join(CATALOG_FILE)).unwrap();
	let mut catalog = AssetCatalog::from_json(&original_catalog).unwrap();
	catalog.assets[0]
		.fingerprint
		.clone_from(&original_asset.source_sha256);
	catalog.assets[0].bytes = original.len() as u64;
	std::fs::write(dir.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	assert!(
		animation_asset::load(&dir, &source)
			.unwrap_err()
			.contains("outside its catalog")
	);
	catalog.assets[0]
		.fingerprint
		.clone_from(&replacement_asset.source_sha256);
	catalog.assets[0].bytes = replacement.len() as u64;
	catalog.assets[0].revision += 1;
	std::fs::write(dir.join(CATALOG_FILE), catalog.to_json().unwrap()).unwrap();
	let metadata = animation_asset::inspect(&dir, &id).unwrap();
	assert!(metadata.joint_metadata.iter().any(
		|joint| joint.name == "Renamed joint" && joint.stable_id.as_deref() == Some("joint-0")
	));
	let mut data = scene(true);
	data.entities[0].animation.as_mut().unwrap().settings.source = source;
	let pins = [(key("clip-demo"), Arc::new(original_asset))].into();
	let bindings = animation_asset::prepare_scene_with_pins(&dir, &data, &pins).unwrap();
	assert_eq!(
		bindings[&key("clip-demo")].source_sha256,
		replacement_asset.source_sha256
	);
	let mut project = ProjectData::for_scene("Character reimport", "scene.json", &data).unwrap();
	project.fixed_hz = 20;
	let mut simulation = SimulationSession::new(project, Arc::new(data)).unwrap();
	simulation.install_animation_bindings(bindings).unwrap();
	simulation.tick(0.05).unwrap();
	let pose = simulation.animations().pose(&key("clip-demo")).unwrap();
	assert_eq!(pose.clip_index, walk);
	assert_eq!(
		pose.local,
		sample_local(&replacement_asset, walk as usize, 0.05).unwrap()
	);
}

#[test]
fn character_profile_rejects_partial_duplicate_and_removed_identities() {
	let original = animation_asset::decode(&edit_glb(|doc, _| mark_character(doc))).unwrap();
	let partial = edit_glb(|doc, _| {
		mark_character(doc);
		doc["animations"][0]
			.as_object_mut()
			.unwrap()
			.remove("extras");
	});
	assert!(
		animation_asset::decode(&partial)
			.unwrap_err()
			.contains("every animation clip")
	);
	let duplicate = edit_glb(|doc, _| {
		mark_character(doc);
		doc["animations"][1]["extras"] = doc["animations"][0]["extras"].clone();
	});
	assert!(
		animation_asset::decode(&duplicate)
			.unwrap_err()
			.contains("unique clip IDs")
	);
	let removed = edit_glb(|doc, _| {
		mark_character(doc);
		doc["animations"].as_array_mut().unwrap().remove(1);
	});
	assert!(
		animation_asset::decode(&removed)
			.unwrap()
			.require_compatible_character(&original)
			.unwrap_err()
			.contains("clip-1")
	);
	assert!(
		decoded()
			.require_compatible_character(&original)
			.unwrap_err()
			.contains("lacks stable")
	);
	let mut reparented = original.clone();
	let child = reparented.joint_node("joint-1").unwrap();
	reparented.nodes[usize::from(child)].parent = None;
	assert!(
		reparented
			.require_compatible_character(&original)
			.unwrap_err()
			.contains("changed its parent")
	);
}

struct TestDirectory<'a>(&'a std::path::Path);
impl Drop for TestDirectory<'_> {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(self.0);
	}
}

fn mark_character(doc: &mut Value) {
	let joints = doc["skins"][0]["joints"].as_array().unwrap().clone();
	for (index, node) in joints.into_iter().enumerate() {
		doc["nodes"][node.as_u64().unwrap() as usize]["extras"] =
			json!({"game_engine_joint_id": format!("joint-{index}")});
	}
	for (index, clip) in doc["animations"]
		.as_array_mut()
		.unwrap()
		.iter_mut()
		.enumerate()
	{
		clip["extras"] = json!({"game_engine_clip_id": format!("clip-{index}")});
	}
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
