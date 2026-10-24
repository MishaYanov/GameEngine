mod support;
use runtime::scene_data::{
	CURRENT_SCHEMA_VERSION, ColliderData, LightData, LightKind, MaterialOverrideData,
	TransformData, parse_json,
};
use serde_json::json;
use support::*;

#[test]
fn v1_and_v2_migrate_one_collision_free_default_light_but_v3_empty_lights_stay_empty() {
	for version in [1, 2] {
		let mut value = serde_json::to_value(scene(vec![entity(
			"legacy-default-directional-light",
			[0.0; 3],
		)]))
		.unwrap();
		value["schema_version"] = json!(version);
		value.as_object_mut().unwrap().remove("lights");
		for e in value["entities"].as_array_mut().unwrap() {
			for field in ["renderable", "parent", "material", "collider", "mesh_index"] {
				e.as_object_mut().unwrap().remove(field);
			}
			if version == 1 {
				e.as_object_mut().unwrap().remove("key");
			}
		}
		if version == 1 {
			value.as_object_mut().unwrap().remove("bindings");
		}
		let parsed = parse_json(&value.to_string()).unwrap();
		assert_eq!(parsed.source_schema_version, version);
		assert_eq!(parsed.data.schema_version, CURRENT_SCHEMA_VERSION);
		assert_eq!(parsed.data.lights.len(), 1);
		assert_eq!(parsed.data.lights[0].kind, LightKind::Directional);
		assert_ne!(parsed.data.entities[0].key, parsed.data.lights[0].key);
		assert_eq!(
			runtime::SceneData::from_json(&parsed.data.to_json().unwrap()).unwrap(),
			parsed.data
		);
	}
	let data = scene(vec![]);
	assert!(
		runtime::SceneData::from_json(&data.to_json().unwrap())
			.unwrap()
			.lights
			.is_empty()
	);
}

#[test]
fn schema3_extras_roundtrip_with_child_before_parent_and_resolve_full_world_trs() {
	let mut parent = entity("parent", [4.0, 2.0, 1.0]);
	parent.transform.rotation[2] = std::f32::consts::FRAC_PI_2;
	parent.transform.scale = [-2.0, 3.0, 1.0];
	parent.renderable = false;
	let mut child = entity("child", [1.0, 0.0, 0.0]).with_parent(Some(parent.key.clone()));
	child.mesh_index = Some(7);
	child.material = Some(MaterialOverrideData {
		base_color_factor: [0.1, 0.2, 0.3, 0.4],
	});
	child.collider = Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.1; 3],
	});
	let data = scene(vec![child.clone(), parent.clone()]);
	assert_eq!(
		runtime::SceneData::from_json(&data.to_json().unwrap()).unwrap(),
		data
	);
	let matrices = data.validated_world_matrices().unwrap();
	matrix_close(
		matrices[0],
		parent.transform.matrix() * child.transform.matrix(),
	);
	matrix_close(matrices[1], parent.transform.matrix());
}

#[test]
fn hierarchy_rejects_cycles_self_parent_missing_parent_and_nonfinite_world_results() {
	let mut data = scene(vec![entity("a", [0.0; 3]), entity("b", [0.0; 3])]);
	for parent in ["a", "missing"] {
		data.entities[0].parent = Some(key(parent));
		assert!(data.validate().is_err());
		assert!(data.world_matrices().is_err());
	}
	data.entities[0].parent = Some(key("b"));
	data.entities[1].parent = Some(key("a"));
	assert!(data.validate().is_err());
	data.entities[1].parent = None;
	data.entities[1].transform.scale = [f32::MAX; 3];
	data.entities[0].transform.scale = [2.0; 3];
	assert!(data.validate().is_err());
}

#[test]
fn trs_decomposition_accepts_reflection_but_rejects_shear_singular_and_perspective() {
	let original = TransformData {
		position: [1.0, 2.0, 3.0],
		rotation: [0.2, -0.3, 0.4],
		scale: [-2.0, 3.0, 4.0],
	};
	matrix_close(
		TransformData::from_matrix(original.matrix())
			.unwrap()
			.matrix(),
		original.matrix(),
	);
	let mut shear = glam::Mat4::IDENTITY;
	shear.y_axis.x = 0.5;
	let mut perspective = glam::Mat4::IDENTITY;
	perspective.x_axis.w = 0.5;
	for invalid in [shear, perspective, glam::Mat4::ZERO] {
		assert!(TransformData::from_matrix(invalid).is_err());
	}
}

#[test]
fn invalid_material_colliders_and_lights_are_rejected_and_keys_share_one_namespace() {
	let base = scene(vec![entity("a", [0.0; 3])]);
	for color in [[-0.1, 0.0, 0.0, 1.0], [1.1; 4], [f32::NAN; 4]] {
		let mut data = base.clone();
		data.entities[0].material = Some(MaterialOverrideData {
			base_color_factor: color,
		});
		assert!(data.validate().is_err());
	}
	for collider in [
		ColliderData::Sphere {
			radius: 0.0,
			offset: [0.0; 3],
		},
		ColliderData::Box {
			half_extents: [1.0, -1.0, 1.0],
			offset: [0.0; 3],
		},
		ColliderData::Sphere {
			radius: 1.0,
			offset: [f32::INFINITY; 3],
		},
	] {
		let mut data = base.clone();
		data.entities[0].collider = Some(collider);
		assert!(data.validate().is_err());
	}
	let light = LightData::new(key("light"), "Light", LightKind::Directional);
	for defect in 0..6 {
		let mut data = base.clone();
		let mut light = light.clone();
		match defect {
			0 => light.key = key("a"),
			1 => light.direction = [0.0; 3],
			2 => light.intensity = -1.0,
			3 => light.range = 0.0,
			4 => light.color = [f32::NAN; 3],
			_ => light.position = [f32::INFINITY; 3],
		}
		data.lights.push(light);
		assert!(data.validate().is_err(), "accepted light defect {defect}");
	}
	let mut data = base;
	data.lights = vec![light; 17];
	assert!(data.validate().is_err());
}
