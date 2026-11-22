mod support;

use engine_core::{
	EntityKey, asset_catalog,
	command::{EditorCommandDto, EditorCommandRequest},
	editor::{EditorSession, PrefabData},
	scene_data,
};
use std::collections::BTreeMap;

use engine_core::linked_prefab::{
	LinkedPrefabSource, MAX_NESTED_PLACEMENTS, MAX_PROP_NESTING, NestedBinding, NestedPlacement,
	PinnedAssetVersion, PinnedPropSource, PlacementId, PropAddress, PropComposition, resolve_scene,
};
use support::{entity, key, scene};

fn version() -> PinnedAssetVersion {
	PinnedAssetVersion {
		asset_id: asset_catalog::AssetId::generate(),
		fingerprint: "6".repeat(64),
	}
}

fn pin(version: PinnedAssetVersion, name: &str) -> PinnedPropSource {
	PinnedPropSource {
		version,
		path: format!("prefabs/{name}.json"),
		entities: vec![entity(name, [0.0; 3])],
		reference_slots: BTreeMap::new(),
		placements: Vec::new(),
	}
}

fn placement(id: &str, target: &PinnedAssetVersion, parent: &str, x: f32) -> NestedPlacement {
	NestedPlacement {
		id: PlacementId::new(id).unwrap(),
		name: id.into(),
		target: target.clone(),
		parent: key(parent),
		local: scene_data::TransformData {
			position: [x, 0.0, 0.0],
			..Default::default()
		},
		bindings: BTreeMap::new(),
	}
}

fn scoped_key(address: &PropAddress) -> Result<EntityKey, String> {
	let (scope, suffix) = match address {
		PropAddress::Object {
			placement_path,
			source_entity,
		} => (placement_path, source_entity.as_str()),
		PropAddress::Placement { placement_path } => (placement_path, ""),
	};
	let name = scope
		.iter()
		.map(PlacementId::as_str)
		.collect::<Vec<_>>()
		.join("/");
	EntityKey::new(format!("{name}:{suffix}"))
}

#[test]
fn repeated_placements_share_a_pin_and_keep_distinct_addresses_and_poses() {
	let owner = version();
	let base = vec![entity("floor", [10.0, 0.0, 0.0])];
	let child = pin(version(), "grate");
	let left = placement("left", &child.version, "floor", 1.0);
	let right = placement("right", &child.version, "floor", 3.0);
	let mut composition = PropComposition {
		pins: vec![child],
		placements: vec![left, right],
	};
	assert!(PropComposition::default().is_empty());
	let encoded = serde_json::to_value(&composition).unwrap();
	assert_eq!(encoded["pins"].as_array().unwrap().len(), 1);
	let decoded: PropComposition = serde_json::from_value(encoded).unwrap();
	assert_eq!(composition, decoded);
	let first = composition
		.flatten(Some(&owner.asset_id), &base, &BTreeMap::new(), scoped_key)
		.unwrap();
	assert_eq!(first.entities.len(), 5);
	let original = scene(first.entities);
	let positions: BTreeMap<_, _> = original
		.entities
		.iter()
		.zip(original.world_matrices().unwrap())
		.map(|(entity, transform)| (entity.key.clone(), transform.w_axis.x))
		.collect();
	assert_eq!(positions[&key("left:grate")], 11.0);
	assert_eq!(positions[&key("right:grate")], 13.0);
	composition.placements.reverse();
	composition.placements[0].name = "Renamed grate".into();
	let second = composition
		.flatten(Some(&owner.asset_id), &base, &BTreeMap::new(), scoped_key)
		.unwrap();
	assert_eq!(first.addresses, second.addresses);
	assert!(
		composition
			.flatten(Some(&owner.asset_id), &base, &BTreeMap::new(), |_| Ok(key(
				"floor"
			)))
			.is_err()
	);
}

#[test]
fn pin_graph_validates_explicit_bindings_and_active_asset_ancestry() {
	let owner = version();
	let base = vec![entity("floor", [0.0; 3])];
	let mut child = pin(version(), "grate");
	child
		.reference_slots
		.insert("User".into(), key("external-user"));
	let mut entry = placement("grate-left", &child.version, "floor", 0.0);
	let mut composition = PropComposition {
		pins: vec![child.clone()],
		placements: vec![entry.clone()],
	};
	assert!(
		composition
			.validate(Some(&owner.asset_id), &base, &BTreeMap::new())
			.unwrap_err()
			.contains("every child slot")
	);
	entry.bindings.insert(
		"User".into(),
		NestedBinding::Slot {
			name: "Player".into(),
		},
	);
	composition.placements[0] = entry;
	let owning_slots = BTreeMap::from([("Player".into(), key("scene-player"))]);
	assert_eq!(
		composition
			.validate(Some(&owner.asset_id), &base, &owning_slots)
			.unwrap(),
		3
	);

	let reentrant = placement("back-to-floor", &owner, "grate", 0.0);
	composition.pins[0].placements.push(reentrant);
	composition.pins.push(pin(owner.clone(), "floor"));
	assert!(
		composition
			.validate(Some(&owner.asset_id), &base, &owning_slots)
			.unwrap_err()
			.contains("source cycle")
	);
	composition.pins[0].placements.clear();
	assert!(
		composition
			.validate(Some(&owner.asset_id), &base, &owning_slots)
			.unwrap_err()
			.contains("unused source version")
	);
	composition.pins.pop();
	composition.pins.push(child);
	assert!(
		composition
			.validate(Some(&owner.asset_id), &base, &owning_slots)
			.unwrap_err()
			.contains("pinned more than once")
	);
}

#[test]
fn expansion_budgets_cover_depth_and_counts_even_when_reusing_one_pin() {
	let base = vec![entity("floor", [0.0; 3])];
	let mut pins: Vec<_> = (0..=MAX_PROP_NESTING)
		.map(|_| pin(version(), "child"))
		.collect();
	for index in 1..pins.len() {
		let target = pins[index].version.clone();
		pins[index - 1]
			.placements
			.push(placement("next", &target, "child", 0.0));
	}
	let mut composition = PropComposition {
		placements: vec![placement("first", &pins[0].version, "floor", 0.0)],
		pins,
	};
	assert!(
		composition
			.validate(None, &base, &BTreeMap::new())
			.unwrap_err()
			.contains("exceeds depth")
	);
	composition.pins.truncate(1);
	composition.pins[0].placements.clear();
	composition.placements = (0..=MAX_NESTED_PLACEMENTS)
		.map(|i| {
			placement(
				&format!("placement-{i}"),
				&composition.pins[0].version,
				"floor",
				0.0,
			)
		})
		.collect();
	assert!(
		composition
			.validate(None, &base, &BTreeMap::new())
			.unwrap_err()
			.contains("Expanded Prop exceeds")
	);
}

#[test]
fn repeated_large_pin_is_rejected_before_expanding_any_frame() {
	let base = vec![entity("floor", [0.0; 3])];
	let mut child = pin(version(), "grate");
	child.entities[0].name = "g".repeat(160 * 1024);
	let placements = (0..MAX_NESTED_PLACEMENTS)
		.map(|index| placement(&format!("grate-{index}"), &child.version, "floor", 0.0))
		.collect();
	let composition = PropComposition {
		placements,
		pins: vec![child],
	};
	assert!(
		composition
			.flatten(None, &base, &BTreeMap::new(), |_| {
				panic!("invalid composition started allocating expanded entities")
			})
			.unwrap_err()
			.contains("serialized entity budget")
	);
}

#[test]
fn saved_nested_sources_link_and_reopen_with_distinct_scene_member_keys() {
	let parent_id = asset_catalog::AssetId::generate();
	let outer = PrefabData {
		schema_version: 3,
		entities: vec![entity("floor", [0.0; 3])],
		gameplay: None,
		reference_slots: BTreeMap::new(),
		composition: PropComposition::default(),
	};
	let child = LinkedPrefabSource {
		source_id: asset_catalog::AssetId::generate(),
		path: "prefabs/grate.json".into(),
		fingerprint: "a".repeat(64),
		prefab: PrefabData {
			schema_version: 3,
			entities: vec![entity("grate", [0.0; 3])],
			gameplay: None,
			reference_slots: BTreeMap::new(),
			composition: PropComposition::default(),
		},
	};
	let target = PinnedAssetVersion {
		asset_id: child.source_id.clone(),
		fingerprint: child.fingerprint.clone(),
	};
	let first = outer
		.with_nested_placement(&parent_id, &child, placement("left", &target, "floor", 1.0))
		.unwrap();
	let staged = first
		.with_nested_placement(
			&parent_id,
			&child,
			placement("right", &target, "floor", 3.0),
		)
		.unwrap();
	assert!(outer.composition.is_empty());
	assert_eq!(staged.composition.pins.len(), 1);
	let file = staged.to_json().unwrap();
	let parsed = PrefabData::from_json(&file).unwrap();
	assert_eq!(parsed.schema_version, 4);
	assert_eq!(parsed, staged);
	assert_eq!(
		PrefabData::from_json(&outer.to_json().unwrap())
			.unwrap()
			.schema_version,
		3
	);

	let mut tampered = child;
	tampered.prefab.entities[0].name = "Different captured contents".into();
	assert!(
		staged
			.with_nested_placement(
				&parent_id,
				&tampered,
				placement("third", &target, "floor", 5.0),
			)
			.unwrap_err()
			.contains("Conflicting captured contents")
	);
	assert_eq!(staged.composition.placements.len(), 2);

	let source = LinkedPrefabSource {
		source_id: parent_id,
		path: "prefabs/floor.json".into(),
		fingerprint: "b".repeat(64),
		prefab: parsed,
	};
	let mut session = EditorSession::new(scene(Vec::new())).unwrap();
	for x in [4.0, 20.0] {
		session
			.execute_command(EditorCommandRequest {
				expected_revision: session.revision(),
				command: EditorCommandDto::InstantiateLinkedPrefab {
					source: source.clone(),
					position: [x, 0.0, 0.0],
					bindings: BTreeMap::new(),
				},
			})
			.unwrap();
	}
	let authored = session.authored_document();
	let reopened = scene_data::parse_json(&authored.to_json().unwrap())
		.unwrap()
		.data;
	assert_eq!(authored.as_ref(), &reopened);
	let maps: Vec<_> = reopened
		.entities
		.iter()
		.filter_map(|entity| entity.linked_prefab.as_ref())
		.map(|instance| &instance.entity_map)
		.collect();
	assert_eq!(maps.len(), 2);
	assert_eq!(maps[0].len(), 5);
	assert!(maps[0].keys().eq(maps[1].keys()));
	assert!(maps[0].keys().all(|key| maps[0][key] != maps[1][key]));
	let runtime = resolve_scene(&reopened);
	assert!(
		runtime
			.entities
			.iter()
			.all(|entity| entity.linked_prefab.is_none())
	);
	let matrices = runtime.world_matrices().unwrap();
	let mut grates: Vec<_> = runtime
		.entities
		.iter()
		.zip(matrices)
		.filter(|(entity, _)| entity.name == "grate")
		.map(|(_, matrix)| matrix.w_axis.x)
		.collect();
	grates.sort_by(f32::total_cmp);
	assert_eq!(grates, vec![5.0, 7.0, 21.0, 23.0]);
}

#[test]
fn refreshing_one_placement_only_releases_pins_unreachable_from_any_placement() {
	fn source(name: &str, fingerprint: &str) -> LinkedPrefabSource {
		LinkedPrefabSource {
			source_id: asset_catalog::AssetId::generate(),
			path: format!("prefabs/{name}.json"),
			fingerprint: fingerprint.repeat(64),
			prefab: PrefabData {
				schema_version: 3,
				entities: vec![entity(name, [0.0; 3])],
				gameplay: None,
				reference_slots: BTreeMap::new(),
				composition: PropComposition::default(),
			},
		}
	}

	let floor = source("floor", "1");
	let bolt = source("bolt", "2");
	let original_grate = source("grate", "3");
	let bolt_version = PinnedAssetVersion {
		asset_id: bolt.source_id.clone(),
		fingerprint: bolt.fingerprint.clone(),
	};
	let mut old_grate = original_grate.clone();
	old_grate.prefab = original_grate
		.prefab
		.with_nested_placement(
			&original_grate.source_id,
			&bolt,
			placement("bolt", &bolt_version, "grate", 1.0),
		)
		.unwrap();
	let old_grate_version = PinnedAssetVersion {
		asset_id: old_grate.source_id.clone(),
		fingerprint: old_grate.fingerprint.clone(),
	};
	let left = placement("left", &old_grate_version, "floor", 2.0);
	let right = placement("right", &old_grate_version, "floor", 4.0);
	let draft = floor
		.prefab
		.with_nested_placement(&floor.source_id, &old_grate, left.clone())
		.unwrap()
		.with_nested_placement(&floor.source_id, &old_grate, right.clone())
		.unwrap();
	assert_eq!(draft.composition.pins.len(), 2);

	let mut new_grate = original_grate.clone();
	new_grate.fingerprint = "4".repeat(64);
	new_grate.prefab.entities[0].name = "Replacement grating".into();
	let new_grate_version = PinnedAssetVersion {
		asset_id: new_grate.source_id.clone(),
		fingerprint: new_grate.fingerprint.clone(),
	};

	let first_refresh = draft
		.with_refreshed_nested_placement(&floor.source_id, &left.id, &new_grate)
		.unwrap();
	assert_eq!(
		first_refresh.composition.placements,
		[
			NestedPlacement {
				target: new_grate_version.clone(),
				..left.clone()
			},
			right.clone(),
		]
	);
	assert_eq!(first_refresh.composition.pins.len(), 3);
	let second_refresh = first_refresh
		.with_refreshed_nested_placement(&floor.source_id, &right.id, &new_grate)
		.unwrap();
	assert_eq!(second_refresh.composition.pins.len(), 1);
	assert_eq!(
		second_refresh.composition.pins[0].version,
		new_grate_version
	);

	let mut requires_new_binding = new_grate.clone();
	requires_new_binding.fingerprint = "5".repeat(64);
	requires_new_binding
		.prefab
		.reference_slots
		.insert("Guide".into(), key("outside-guide"));
	let before = first_refresh.to_json().unwrap();
	assert!(
		first_refresh
			.with_refreshed_nested_placement(&floor.source_id, &left.id, &requires_new_binding)
			.unwrap_err()
			.contains("every child slot")
	);
	assert!(
		first_refresh
			.with_refreshed_nested_placement(&floor.source_id, &left.id, &bolt)
			.unwrap_err()
			.contains("same nested Prop asset identity")
	);
	assert_eq!(first_refresh.to_json().unwrap(), before);
}
