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
