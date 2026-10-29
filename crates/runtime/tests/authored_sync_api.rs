mod support;
use runtime::{
	Camera, Transform, assets::Model, model_instance::ModelInstance, scene::Scene,
	world::WorldChange,
};
use std::sync::Arc;
use support::*;

fn native(data: &runtime::SceneData) -> Scene {
	let mut scene = Scene::default();
	let model = Arc::new(Model::new(vec![]));
	for entity in &data.entities {
		let id = scene
			.spawn_keyed_model(
				entity.key.clone(),
				&entity.name,
				&entity.model_path,
				ModelInstance::new(Arc::clone(&model), Transform::default()),
			)
			.unwrap();
		scene.set_entity_metadata(id, entity).unwrap();
	}
	assert!(scene.synchronize(data).unwrap());
	scene
}

#[test]
fn authored_updates_descendants_in_any_order_and_preserves_ids_resources_and_undo() {
	let original = scene(vec![
		entity("child", [1.0, 0.0, 0.0]).with_parent(Some(key("parent"))),
		entity("parent", [0.0; 3]),
		entity("unrelated", [9.0; 3]),
	]);
	let mut native = native(&original);
	let ids = native.entity_ids().collect::<Vec<_>>();
	let model = native.get(ids[0]).unwrap().model() as *const Model;
	let mut next = original.clone();
	next.entities[1].transform.position = [2.0, 3.0, 4.0];
	next.entities[1].transform.rotation[2] = 0.5;
	let parent_only = WorldChange {
		changed_entities: vec![key("parent")],
		..Default::default()
	};
	assert!(
		native
			.synchronize_authored(next.validated().unwrap(), &parent_only)
			.unwrap()
	);
	for (id, expected) in ids.iter().zip(next.world_matrices().unwrap()) {
		matrix_close(native.world_matrix(*id).unwrap(), expected);
	}
	next.entities[0].transform.position[0] = 2.0;
	let changes = WorldChange {
		changed_entities: vec![key("child"), key("parent"), key("parent")],
		..Default::default()
	};
	for data in [&next, &original] {
		assert!(
			native
				.synchronize_authored(data.validated().unwrap(), &changes)
				.unwrap()
		);
		for (id, expected) in ids.iter().zip(data.world_matrices().unwrap()) {
			matrix_close(native.world_matrix(*id).unwrap(), expected);
		}
		assert_eq!(native.entity_ids().collect::<Vec<_>>(), ids);
		assert_eq!(native.get(ids[0]).unwrap().model() as *const Model, model);
	}
}

#[test]
fn metadata_bindings_and_camera_only_updates_preserve_world_matrices() {
	let mut data = scene(vec![entity("a", [2.0, 3.0, 4.0])]);
	let mut native = native(&data);
	let id = native.entity_ids().next().unwrap();
	let matrix = native.world_matrix(id).unwrap();
	data.entities[0].name = "Renamed".into();
	let changes = WorldChange {
		changed_entities: vec![key("a")],
		..Default::default()
	};
	assert!(
		native
			.synchronize_authored(data.validated().unwrap(), &changes)
			.unwrap()
	);
	assert_eq!(native.name(id), Some("Renamed"));
	data.bindings.insert("actor".into(), key("a"));
	data.camera.position[0] += 1.0;
	assert!(
		native
			.synchronize_authored(
				data.validated().unwrap(),
				&WorldChange {
					camera_changed: true,
					..Default::default()
				}
			)
			.unwrap()
	);
	assert_eq!(native.binding("actor"), Some(id));
	assert_eq!(native.world_matrix(id).unwrap(), matrix);
}

#[test]
fn fallbacks_reject_before_publishing_partial_changes() {
	let data = scene(vec![entity("a", [0.0; 3]), entity("b", [0.0; 3])]);
	let mut native = native(&data);
	let before = native.to_data(&Camera::default()).unwrap();
	let mut next = data.clone();
	next.entities[0].name = "Must not leak".into();
	next.entities[1].parent = Some(key("a"));
	let changes = WorldChange {
		changed_entities: vec![key("a"), key("b")],
		..Default::default()
	};
	assert!(
		!native
			.synchronize_authored(next.validated().unwrap(), &changes)
			.unwrap()
	);
	assert_eq!(native.to_data(&Camera::default()).unwrap(), before);
	for changes in [
		WorldChange {
			changed_entities: vec![key("missing")],
			..Default::default()
		},
		WorldChange {
			full_resync: true,
			..Default::default()
		},
		WorldChange {
			topology_changed: true,
			..Default::default()
		},
	] {
		assert!(
			!native
				.synchronize_authored(data.validated().unwrap(), &changes)
				.unwrap()
		);
		assert_eq!(native.to_data(&Camera::default()).unwrap(), before);
	}
	next.entities[1].parent = None;
	next.entities[1].model_path = "replacement.glb".into();
	assert!(
		!native
			.synchronize_authored(next.validated().unwrap(), &changes)
			.unwrap()
	);
	next.entities.swap(0, 1);
	assert!(
		!native
			.synchronize_authored(next.validated().unwrap(), &changes)
			.unwrap()
	);
}
