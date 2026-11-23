use runtime::assets::gltf_loader;
use serde_json::json;

use super::{Directory, model, write};

#[test]
fn default_scene_import_retains_hierarchy_mesh_identity_local_trs_and_world_transforms() {
	let directory = Directory::new("gltf-scene");
	let path = write(&directory, &model());
	let imported = gltf_loader::load_scene(path).unwrap();
	assert_eq!(imported.scene_index, Some(1));
	assert_eq!(imported.root_node_indices, vec![1]);
	assert_eq!(
		imported
			.nodes
			.iter()
			.map(|n| n.node_index)
			.collect::<Vec<_>>(),
		vec![1, 2, 3]
	);
	assert_eq!(imported.nodes[0].mesh_index, None);
	assert_eq!(imported.nodes[1].mesh_index, Some(1));
	assert_eq!(imported.nodes[2].mesh_index, Some(1));
	assert_eq!(imported.nodes[1].parent_node_index, Some(1));
	assert_eq!(imported.nodes[1].local_transform.position, [0.0, 2.0, 0.0]);
	let matrix = glam::Mat4::from_cols_array(&imported.nodes[1].world_matrix);
	assert_eq!(matrix.w_axis.truncate().to_array(), [4.0, 2.0, 0.0]);
	assert_eq!(imported.mesh_count, 2);
	assert_eq!(imported.mesh_bounds.len(), 2);
	let bounds = imported.mesh_bounds[1].unwrap();
	assert_eq!(bounds.min(), [0.0; 3]);
	assert_eq!(bounds.max(), [1.0, 1.0, 0.0]);
}

#[test]
fn missing_default_selects_first_scene_and_missing_scene_is_an_explicit_error() {
	let directory = Directory::new("gltf-default");
	let mut value = model();
	value.as_object_mut().unwrap().remove("scene");
	let imported = gltf_loader::load_scene(write(&directory, &value)).unwrap();
	assert_eq!(imported.scene_index, Some(0));
	assert_eq!(imported.nodes.len(), 1);
	assert_eq!(imported.nodes[0].name, "Unused");
	value.as_object_mut().unwrap().remove("scenes");
	assert!(gltf_loader::load_scene(write(&directory, &value)).is_err());
}

#[test]
fn static_import_rejects_cycles_shared_children_and_unrepresentable_node_transforms() {
	let directory = Directory::new("gltf-invalid");
	for nodes in [
		json!([{"children":[1]},{"children":[0]}]),
		json!([{"children":[1,2]},{"children":[2]},{}]),
		json!([{"matrix":[1,0,0,0,0.5,1,0,0,0,0,1,0,0,0,0,1]}]),
		json!([{"matrix":[0,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]),
		json!([{"rotation":[0,0,0,2]}]),
	] {
		let value =
			json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":nodes});
		assert!(
			gltf_loader::load_scene(write(&directory, &value)).is_err(),
			"accepted {value}"
		);
	}
}

#[test]
fn static_import_bounds_depth_and_node_count_before_recursive_or_unbounded_work() {
	let directory = Directory::new("gltf-budget");
	let nodes = (0..130)
		.map(|i| {
			if i == 129 {
				json!({})
			} else {
				json!({"children":[i+1]})
			}
		})
		.collect::<Vec<_>>();
	let mut value = json!({"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":nodes});
	assert!(
		gltf_loader::load_scene(write(&directory, &value))
			.unwrap_err()
			.to_string()
			.contains("128")
	);
	value["nodes"] = json!(vec![json!({}); 10001]);
	assert!(
		gltf_loader::load_scene(write(&directory, &value))
			.unwrap_err()
			.to_string()
			.contains("10000")
	);
}

#[test]
fn starter_primitives_import_with_expected_geometry_and_no_external_files() {
	use engine_core::PrimitiveKind;
	use glam::Vec3;
	use std::{fs, path::Path};

	let models = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../apps/editor-host/src-tauri/assets/primitives");
	let directory = Directory::new("starter-primitives");
	for kind in PrimitiveKind::ALL {
		let filename = format!("{}.glb", kind.as_str());
		let bytes = fs::read(models.join(&filename)).unwrap();
		let glb = gltf::Gltf::from_slice(&bytes).unwrap();
		assert!(glb.blob.is_some());
		assert_eq!(glb.buffers().len(), 1);
		assert!(matches!(
			glb.buffers().next().unwrap().source(),
			gltf::buffer::Source::Bin
		));
		assert_eq!(glb.images().len(), 0);

		// Import the model from an otherwise empty asset root.
		let path = directory.0.join(&filename);
		fs::write(&path, &bytes).unwrap();
		let scene = gltf_loader::load_scene(&path).unwrap();
		assert_eq!(scene.referenced_bytes, bytes.len());
		assert_eq!(scene.root_node_indices, vec![0]);
		assert_eq!(scene.nodes.len(), 1);
		assert_eq!(scene.nodes[0].name, kind.display_name());
		assert_eq!(
			scene.nodes[0].local_transform,
			engine_core::Transform::default()
		);
		assert_eq!(scene.mesh_count, 1);
		let (min, max) = match kind {
			PrimitiveKind::Plane => ([-0.5, 0.0, -0.5], [0.5, 0.0, 0.5]),
			PrimitiveKind::Cube | PrimitiveKind::Sphere => ([-0.5; 3], [0.5; 3]),
			PrimitiveKind::Capsule => ([-0.5, -1.0, -0.5], [0.5, 1.0, 0.5]),
		};
		let bounds = scene.mesh_bounds[0].unwrap();
		assert_eq!(bounds.min(), min);
		assert_eq!(bounds.max(), max);

		let primitives = gltf_loader::load_first_mesh(&path).unwrap();
		assert_eq!(primitives.len(), 1);
		let primitive = &primitives[0];
		assert_eq!(primitive.material.base_color_factor, [0.7, 0.7, 0.7, 1.0]);
		assert!(!primitive.material.double_sided);
		let vertices = primitive.mesh.vertices();
		let indices = primitive.mesh.indices();
		assert!(!vertices.is_empty() && !indices.is_empty());
		assert_eq!(indices.len() % 3, 0);
		for vertex in vertices {
			assert!(Vec3::from_array(vertex.position).is_finite());
			assert!((Vec3::from_array(vertex.normal).length() - 1.0).abs() < 1e-4);
			assert!(vertex.uv.into_iter().all(|uv| (0.0..=1.0).contains(&uv)));
		}
		for triangle in indices.chunks_exact(3) {
			let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
				.map(|index| Vec3::from_array(vertices[index as usize].position));
			let outside = if kind == PrimitiveKind::Plane {
				Vec3::Y
			} else {
				a + b + c
			};
			assert!((b - a).cross(c - a).dot(outside) > 0.0, "{kind:?} winding");
		}
		fs::remove_file(path).unwrap();
	}
}
