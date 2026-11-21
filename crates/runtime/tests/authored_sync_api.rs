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

fn orbit_scene() -> runtime::SceneData {
	use runtime::scene_data::OrbitCameraSettings;
	let mut data = scene(vec![entity("actor", [0.; 3]), entity("prop", [1.; 3])]);
	data.camera.orbit = Some(OrbitCameraSettings {
		target: key("actor"),
		pivot_offset: [0., 0.7, 0.],
		initial_yaw: 0.2,
		initial_pitch: -0.3,
		initial_distance: 5.,
		min_distance: 1.,
		max_distance: 8.,
		min_pitch: -1.,
		max_pitch: 0.5,
		follow_seconds: 0.1,
		recovery_seconds: 0.2,
		teleport_distance: 8.,
		probe_radius: 0.25,
		obstruction_mask: 4,
	});
	data
}

#[test]
fn orbit_settings_survive_native_sync_and_save_including_mode_changes_and_rebuilds() {
	use runtime::scene_data::FollowCameraSettings;
	use runtime::world::RuntimeWorld;

	let camera = Camera::default();
	let mut data = orbit_scene();
	let mut native = native(&data);
	let actor = native.entity_by_key(&key("actor")).unwrap();
	assert_eq!(native.to_data(&camera).unwrap(), data);
	assert!(native.matches_data(&data, &camera));

	// A camera-only revision can change settings without changing the fallback pose.
	data.camera.orbit.as_mut().unwrap().initial_distance = 6.;
	assert!(!native.matches_data(&data, &camera));
	let camera_changed = WorldChange {
		camera_changed: true,
		..Default::default()
	};
	assert!(
		native
			.synchronize_authored(data.validated().unwrap(), &camera_changed)
			.unwrap()
	);
	assert_eq!(native.to_data(&camera).unwrap(), data);

	// Remove a different object to force the retained-resource rebuild.
	data.entities.pop();
	let world = RuntimeWorld::new(Arc::new(data.clone())).unwrap();
	let structural = WorldChange {
		topology_changed: true,
		..Default::default()
	};
	assert!(native.synchronize_world(&world, &structural).unwrap());
	assert_eq!(native.entity_by_key(&key("actor")), Some(actor));
	let saved = native.to_data(&camera).unwrap().to_json().unwrap();
	assert_eq!(runtime::SceneData::from_json(&saved).unwrap(), data);

	// A Play sync switching modes must clear the previous mode.
	data.camera.orbit = None;
	data.camera.follow = Some(FollowCameraSettings {
		target: key("actor"),
		offset: [0., 2., 5.],
		look_offset: [0.; 3],
		smoothing_seconds: 0.2,
	});
	let world = RuntimeWorld::new(Arc::new(data.clone())).unwrap();
	assert!(native.synchronize_world(&world, &camera_changed).unwrap());
	assert!(native.camera_orbit().is_none());
	assert_eq!(native.to_data(&camera).unwrap(), data);
}

#[test]
fn native_removal_and_restore_keep_the_target_rig_and_reject_camera_conflicts_atomically() {
	let camera = Camera::default();
	let data = orbit_scene();
	let mut native = native(&data);
	let actor = native.entity_by_key(&key("actor")).unwrap();
	let snapshot = native.snapshot(actor).unwrap();
	native.remove(actor).unwrap();
	assert!(native.to_data(&camera).unwrap().camera.orbit.is_none());

	let mut replacement_rig = data.camera.orbit.clone().unwrap();
	replacement_rig.target = key("prop");
	native.set_camera_orbit(Some(replacement_rig));
	let before_conflict = native.to_data(&camera).unwrap();
	assert!(native.restore(&snapshot).is_err());
	assert_eq!(native.to_data(&camera).unwrap(), before_conflict);

	native.set_camera_orbit(None);
	native.restore(&snapshot).unwrap();
	assert_eq!(native.to_data(&camera).unwrap(), data);
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
