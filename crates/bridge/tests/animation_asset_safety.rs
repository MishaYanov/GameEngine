//! Malformed accessors must return diagnostics before entering glTF's typed iterators.
use bridge::animation_asset;
use serde_json::{Value, json};

const GLB: &[u8] = include_bytes!("../../../apps/game/assets/characters/two-joint.glb");
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
fn attribute(doc: &Value, semantic: &str) -> usize {
	doc["meshes"][0]["primitives"][0]["attributes"][semantic]
		.as_u64()
		.unwrap() as usize
}
fn rejected_without_panic(name: &str, bytes: &[u8]) {
	let result = std::panic::catch_unwind(|| animation_asset::decode(bytes));
	assert!(
		result.is_ok(),
		"{name}: malformed source panicked instead of returning an error"
	);
	assert!(
		result.unwrap().is_err(),
		"{name}: malformed source was admitted"
	);
}

#[test]
fn short_stride_zero_count_and_overflowing_ranges_are_rejected_before_readers() {
	for case in [
		"short_stride",
		"zero_animation_count",
		"accessor_overflow",
		"view_overflow",
		"unaligned_offset",
		"stride_misalignment",
	] {
		let bytes = edit_glb(|doc, _| {
			let position = attribute(doc, "POSITION");
			let view = doc["accessors"][position]["bufferView"].as_u64().unwrap() as usize;
			match case {
				"short_stride" => doc["bufferViews"][view]["byteStride"] = json!(4),
				"zero_animation_count" => {
					let input = doc["animations"][0]["samplers"][0]["input"]
						.as_u64()
						.unwrap() as usize;
					let output = doc["animations"][0]["samplers"][0]["output"]
						.as_u64()
						.unwrap() as usize;
					doc["accessors"][input]["count"] = json!(0);
					doc["accessors"][output]["count"] = json!(0);
				}
				"accessor_overflow" => {
					doc["accessors"][position]["byteOffset"] = json!(u64::MAX - 3)
				}
				"view_overflow" => doc["bufferViews"][view]["byteOffset"] = json!(u64::MAX - 3),
				"unaligned_offset" => doc["accessors"][position]["byteOffset"] = json!(1),
				"stride_misalignment" => doc["bufferViews"][view]["byteStride"] = json!(13),
				_ => unreachable!(),
			}
		});
		rejected_without_panic(case, &bytes);
	}
}

#[test]
fn reader_semantic_types_are_checked_in_addition_to_generic_accessor_layout() {
	for case in [
		"position_integer",
		"normal_scalar",
		"inverse_bind_vec4",
		"indices_float",
		"time_vector",
		"rotation_vec3",
	] {
		let bytes = edit_glb(|doc, _| {
			let index = match case {
				"position_integer" => attribute(doc, "POSITION"),
				"normal_scalar" => attribute(doc, "NORMAL"),
				"inverse_bind_vec4" => {
					doc["skins"][0]["inverseBindMatrices"].as_u64().unwrap() as usize
				}
				"indices_float" => doc["meshes"][0]["primitives"][0]["indices"]
					.as_u64()
					.unwrap() as usize,
				"time_vector" => doc["animations"][0]["samplers"][0]["input"]
					.as_u64()
					.unwrap() as usize,
				"rotation_vec3" => doc["animations"][0]["samplers"][0]["output"]
					.as_u64()
					.unwrap() as usize,
				_ => unreachable!(),
			};
			match case {
				"position_integer" => doc["accessors"][index]["componentType"] = json!(5123),
				"normal_scalar" => doc["accessors"][index]["type"] = json!("SCALAR"),
				"inverse_bind_vec4" => doc["accessors"][index]["type"] = json!("VEC4"),
				"rotation_vec3" => doc["accessors"][index]["type"] = json!("VEC3"),
				"indices_float" => doc["accessors"][index]["componentType"] = json!(5126),
				"time_vector" => doc["accessors"][index]["type"] = json!("VEC2"),
				_ => unreachable!(),
			}
		});
		rejected_without_panic(case, &bytes);
	}
}

#[test]
fn huge_finite_normals_cannot_become_zero_or_nan_in_uploaded_geometry() {
	let bytes = edit_glb(|doc, bin| {
		let accessor = attribute(doc, "NORMAL");
		let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
		let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize
			+ doc["accessors"][accessor]["byteOffset"]
				.as_u64()
				.unwrap_or(0) as usize;
		for (i, value) in [f32::MAX, 0., 0.].into_iter().enumerate() {
			bin[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
		}
	});
	let result = std::panic::catch_unwind(|| animation_asset::decode(&bytes));
	assert!(result.is_ok());
	if let Ok(asset) = result.unwrap() {
		for vertex in asset.primitives.iter().flat_map(|p| &p.vertices) {
			let normal = glam::Vec3::from_array(vertex.normal);
			assert!(normal.is_finite() && (normal.length_squared() - 1.).abs() < 1e-4);
		}
	}
}

#[test]
fn material_base_color_factors_must_stay_inside_the_supported_linear_range() {
	for factor in [-0.01_f32, 1.01, f32::MAX] {
		let bytes = edit_glb(|doc, _| {
			doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] =
				json!([factor, 1., 1., 1.]);
		});
		rejected_without_panic("material_factor", &bytes);
	}
}

#[test]
fn finite_world_bounds_cannot_hide_overflow_in_mesh_local_skinning() {
	let bytes = edit_glb(|doc, bin| {
		let accessor = attribute(doc, "POSITION");
		let view = doc["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
		let offset = doc["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize
			+ doc["accessors"][accessor]["byteOffset"]
				.as_u64()
				.unwrap_or(0) as usize;
		bin[offset..offset + 4].copy_from_slice(&1e35_f32.to_le_bytes());
		doc["accessors"][accessor]["max"][0] = json!(1e35_f32);
		doc["nodes"][3]["scale"] = json!([0.0001, 0.0001, 0.0001]);
	});
	let decoded = std::panic::catch_unwind(|| animation_asset::decode(&bytes));
	assert!(decoded.is_ok());
	if let Ok(asset) = decoded.unwrap()
		&& let Ok(pose) = engine_core::animation::sample_pose(
			std::sync::Arc::new(asset),
			0,
			0.,
			glam::Mat4::IDENTITY,
			0,
		) {
		for vertex in pose.asset.primitives.iter().flat_map(|p| &p.vertices) {
			let input = glam::Vec3::from_array(vertex.position).extend(1.);
			let mut skinned = glam::Vec4::ZERO;
			for i in 0..4 {
				// Preserve the shader's actual operation order, including zero-weight lanes.
				skinned +=
					vertex.weights[i] * (pose.palette[usize::from(vertex.joints[i])] * input);
			}
			assert!(
				(pose.mesh_matrix * skinned).is_finite(),
				"accepted world bounds concealed mesh-local overflow"
			);
		}
	}
}
