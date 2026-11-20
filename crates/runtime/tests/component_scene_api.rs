mod support;
use runtime::{
	Camera, Transform,
	assets::Model,
	components::{ComponentId, NativeBehaviorComponent, NativeBehaviorSpec},
	model_instance::ModelInstance,
	scene::Scene,
	scene_data::{ColliderData, ColliderInteraction},
};
use std::sync::Arc;
use support::*;

#[test]
fn native_scene_roundtrip_sync_and_legacy_duplicate_preserve_component_identity() {
	let mut actor = entity("actor", [0.; 3]);
	actor.collider = Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.; 3],
		interaction: Default::default(),
	});
	let mut collectible = entity("collectible", [1., 0., 0.]);
	collectible.collider = Some(ColliderData::Sphere {
		radius: 0.5,
		offset: [0.; 3],
		interaction: ColliderInteraction {
			sensor: true,
			..Default::default()
		},
	});
	collectible.native_behaviors.push(NativeBehaviorComponent {
		id: ComponentId::new("pickup").unwrap(),
		enabled: true,
		behavior: NativeBehaviorSpec::Collectible {
			actor: key("actor"),
			trigger: key("collectible"),
			visual: None,
			despawn_owner: false,
		},
	});
	let data = scene(vec![actor, collectible]);
	let mut native = Scene::default();
	let model = Arc::new(Model::new(vec![]));
	for entity in &data.entities {
		let id = native
			.spawn_keyed_model(
				entity.key.clone(),
				entity.name.clone(),
				entity.model_path.clone(),
				ModelInstance::new(model.clone(), Transform::default()),
			)
			.unwrap();
		native.set_entity_metadata(id, entity).unwrap();
	}
	assert!(native.synchronize(&data).unwrap());
	assert_eq!(native.to_data(&Camera::default()).unwrap(), data);
	let duplicate = native
		.duplicate(
			native.entity_by_key(&key("collectible")).unwrap(),
			[1., 0., 0.],
		)
		.unwrap();
	let copied_key = native.key(duplicate).unwrap();
	let roundtrip = native.to_data(&Camera::default()).unwrap();
	let copy = roundtrip
		.entities
		.iter()
		.find(|entity| &entity.key == copied_key)
		.unwrap();
	assert_eq!(copy.native_behaviors[0].id.as_str(), "pickup");
	let NativeBehaviorSpec::Collectible { actor, trigger, .. } = &copy.native_behaviors[0].behavior
	else {
		panic!("collectible retained")
	};
	assert_eq!(actor, &key("actor"));
	assert_eq!(trigger, copied_key);
	assert_eq!(
		runtime::SceneData::from_json(&roundtrip.to_json().unwrap()).unwrap(),
		roundtrip
	);
}
