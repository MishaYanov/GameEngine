use super::*;
use crate::editor::PrefabData;
use crate::scene_data::{CURRENT_SCHEMA_VERSION, CameraData, SceneData};

fn key(value: &str) -> EntityKey {
	EntityKey::new(value).unwrap()
}
fn component(value: &str) -> ComponentId {
	ComponentId::new(value).unwrap()
}
fn lifecycle() -> RuntimeLifecycle {
	let camera = crate::Camera::default();
	let world = RuntimeWorld::new(Arc::new(SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		organization: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		bindings: BTreeMap::new(),
		entities: vec![EntityData::new(key("owner"), "Owner", "models/item.gltf")],
		camera: CameraData {
			follow: None,
			orbit: None,
			position: camera.position,
			yaw: camera.yaw,
			pitch: camera.pitch,
			vertical_fov: camera.vertical_fov,
			near_plane: camera.near,
			far_plane: camera.far,
		},
		lights: vec![],
	}))
	.unwrap();
	RuntimeLifecycle::new(&world)
}
// Exercise private admission bookkeeping directly; public API tests cover
// prefab/world validation and admission of the matching world candidate.
fn prepared(
	state: &mut RuntimeLifecycle,
	owner: Option<(&EntityKey, &ComponentId)>,
	count: usize,
) -> PreparedRuntimeSpawn {
	let request = owner.map(|(owner, component)| {
		let id = state.next_request;
		state
			.enqueue(
				owner,
				component,
				"prefabs/item.json",
				&BTreeMap::new(),
				[0.; 3],
				10,
				None,
			)
			.unwrap();
		state.requests[&id].clone()
	});
	PreparedRuntimeSpawn {
		world_id: state.world_id.clone(),
		topology_revision: state.topology_revision,
		instance_serial: state.next_instance,
		entities: (0..count)
			.map(|index| {
				EntityData::new(
					key(&format!("runtime-{}-{index}", state.next_instance)),
					"Item",
					"models/item.gltf",
				)
			})
			.collect(),
		request,
		source_fingerprint: "a".repeat(64),
		visible: None,
	}
}
fn admit(
	state: &mut RuntimeLifecycle,
	owner: Option<(&EntityKey, &ComponentId)>,
	count: usize,
) -> Vec<EntityKey> {
	let prepared = prepared(state, owner, count);
	state.admit(&prepared, 0).unwrap();
	prepared
		.entities
		.into_iter()
		.map(|entity| entity.key)
		.collect()
}

#[test]
fn tick_clones_share_ownership_and_structural_changes_detach_all_indexes() {
	let mut original = lifecycle();
	let owner = key("owner");
	let component = component("emitter");
	let first = admit(&mut original, Some((&owner, &component)), 16);
	let mut transaction = original.clone();
	assert!(Arc::ptr_eq(&original.instances, &transaction.instances));
	assert!(Arc::ptr_eq(&original.generations, &transaction.generations));
	let second = admit(&mut transaction, Some((&owner, &component)), 16);
	assert!(!Arc::ptr_eq(&original.instances, &transaction.instances));
	assert_eq!(original.alive_for(&owner, &component), 1);
	assert_eq!(transaction.alive_for(&owner, &component), 2);
	assert!(original.instance_for(&second[0]).is_none());
	assert!(original.handle(&second[0]).is_none());
	let handle = transaction.handle(&first[0]).unwrap();
	let removed = transaction.removal(&handle).unwrap();
	transaction.removed(&removed).unwrap();
	assert_eq!(transaction.alive_for(&owner, &component), 1);
	assert_eq!(original.instance_for(&first[15]), Some("spawn-1"));
	assert_eq!(transaction.instance_for(&second[15]), Some("spawn-2"));
	assert!(transaction.validate_handle(&handle).is_err());
	assert!(original.validate_handle(&handle).is_ok());
}

#[test]
fn removal_visits_child_owners_and_cleans_counts_timers_requests_and_states() {
	let mut state = lifecycle();
	let owner = key("owner");
	let emitter = component("emitter");
	let alternate = component("alternate");
	let parent = admit(&mut state, Some((&owner, &emitter)), 2);
	let child = admit(&mut state, Some((&parent[1], &emitter)), 2);
	let grandchild = admit(&mut state, Some((&child[1], &alternate)), 1);
	let sibling = admit(&mut state, Some((&owner, &alternate)), 1);
	let unowned = admit(&mut state, None, 1);
	state
		.enqueue(
			&parent[1],
			&alternate,
			"prefabs/item.json",
			&BTreeMap::new(),
			[0.; 3],
			1,
			None,
		)
		.unwrap();
	state.states.insert(
		(child[1].clone(), emitter.clone()),
		NativeBehaviorState::default(),
	);
	let snapshot = state.clone();
	let removed = state.removal(&state.handle(&parent[1]).unwrap()).unwrap();
	assert_eq!(removed.len(), 5);
	assert!(removed.contains(&grandchild[0]));
	state.removed(&removed).unwrap();
	assert_eq!(state.alive_for(&owner, &emitter), 0);
	assert_eq!(state.alive_for(&owner, &alternate), 1);
	assert_eq!(state.alive_for(&parent[1], &emitter), 0);
	assert_eq!(state.timer_count(), 1);
	assert_eq!(state.request_count(), 0);
	assert!(state.states.is_empty());
	assert_eq!(state.instances.by_id.len(), 2);
	assert_eq!(state.instances.by_entity.len(), 2);
	assert_eq!(state.instances.by_owner.len(), 1);
	assert!(state.instance_for(&sibling[0]).is_some());
	assert!(state.instance_for(&unowned[0]).is_some());
	assert_eq!(snapshot.alive_for(&owner, &emitter), 1);
	assert_eq!(snapshot.timer_count(), 4);
	assert_eq!(snapshot.request_count(), 1);
}

#[test]
fn ownership_traversal_handles_the_instance_budget_without_recursive_scans() {
	let mut state = lifecycle();
	let emitter = component("emitter");
	let mut owner = key("owner");
	let mut root = None;
	for _ in 0..MAX_RUNTIME_INSTANCES {
		owner = admit(&mut state, Some((&owner, &emitter)), 1).remove(0);
		root.get_or_insert_with(|| owner.clone());
	}
	let removed = state
		.removal(&state.handle(&root.unwrap()).unwrap())
		.unwrap();
	assert_eq!(removed.len(), MAX_RUNTIME_INSTANCES);
	state.removed(&removed).unwrap();
	assert!(!state.has_instances());
	assert!(state.instances.by_entity.is_empty());
	assert!(state.instances.by_owner.is_empty());
	assert_eq!(state.timer_count(), 0);
}

#[test]
fn visibility_membership_accepts_large_valid_subsets_and_preserves_on_rejection() {
	let mut state = lifecycle();
	let mut prepared = prepared(&mut state, None, PrefabData::MAX_ENTITIES);
	let available: HashSet<_> = prepared
		.entities
		.iter()
		.map(|entity| entity.key.clone())
		.collect();
	prepared
		.set_visibility_capabilities(available.clone())
		.unwrap();
	let mut invalid = available.clone();
	invalid.insert(key("foreign"));
	assert!(prepared.set_visibility_capabilities(invalid).is_err());
	assert_eq!(prepared.visible, Some(available));
	prepared
		.set_visibility_capabilities(HashSet::new())
		.unwrap();
	assert_eq!(prepared.visible, Some(HashSet::new()));
}
