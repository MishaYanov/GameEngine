mod support;

use engine_core::{
	EntityKey, asset_catalog,
	command::{
		EditorCommandDto, EditorCommandRequest, TransformDto, TransformEditDto, TransformSpace,
	},
	components::{
		ComponentId, NativeBehaviorComponent, NativeBehaviorSpec, ScriptComponent, ScriptReference,
		ScriptSettings,
	},
	editor::{EditorSession, PrefabData},
	scene_data,
};
use std::collections::BTreeMap;

use engine_core::linked_prefab::{
	LinkedPrefabInstance, LinkedPrefabOverrideResolution, LinkedPrefabProperty, LinkedPrefabSource,
	LinkedPrefabValue, MAX_NESTED_PLACEMENTS, MAX_PROP_NESTING, NestedBinding, NestedPlacement,
	PinnedAssetVersion, PinnedPropSource, PlacementId, PrefabConflictResolution, PropAddress,
	PropComposition, override_status, resolve_scene, review_recovery_source,
	review_source_overrides,
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
fn repeated_nested_scripts_keep_their_local_sensor_and_bind_named_external_slots() {
	let base = vec![entity("floor", [0.0; 3])];
	let mut child = pin(version(), "door");
	child.entities.push(entity("sensor", [0.0; 3]));
	child
		.reference_slots
		.insert("External".into(), key("placeholder"));
	child.entities[0].scripts.push(ScriptComponent {
		id: ComponentId::new("door-script").unwrap(),
		enabled: true,
		settings: ScriptSettings {
			script_id: "test.door".into(),
			config_version: 1,
			properties: BTreeMap::new(),
			references: BTreeMap::from([
				(
					"sensor".into(),
					ScriptReference::Entity {
						entity: key("sensor"),
					},
				),
				(
					"out".into(),
					ScriptReference::Slot {
						slot: "External".into(),
					},
				),
			]),
		},
	});
	let slots = BTreeMap::from([("Outside".into(), key("outside"))]);
	let placements = ["left", "right"].map(|side| {
		let mut place = placement(side, &child.version, "floor", 0.0);
		place.bindings.insert(
			"External".into(),
			NestedBinding::Slot {
				name: "Outside".into(),
			},
		);
		place
	});
	let prefab = PrefabData {
		schema_version: 4,
		entities: base,
		reference_slots: slots,
		gameplay: None,
		composition: PropComposition {
			pins: vec![child],
			placements: placements.into(),
		},
	};
	let reopened = PrefabData::from_json(&prefab.to_json().unwrap()).unwrap();
	assert_eq!(
		reopened.composition.pins[0].entities[0].scripts[0]
			.settings
			.references["out"],
		ScriptReference::Slot {
			slot: "External".into()
		}
	);
	let expanded = reopened.expanded_entities().unwrap();
	let mapping = expanded
		.iter()
		.map(|entity| {
			(
				entity.key.clone(),
				key(&format!("copy-{}", entity.key.as_str())),
			)
		})
		.collect();
	let copied = reopened
		.remap_entities(
			&mapping,
			&BTreeMap::from([("Outside".into(), key("scene-outside"))]),
		)
		.unwrap();
	for (original, clone) in expanded
		.iter()
		.zip(copied)
		.filter(|(entity, _)| !entity.scripts.is_empty())
	{
		let old = &original.scripts[0].settings.references;
		let ScriptReference::Entity { entity: sensor } = &old["sensor"] else {
			panic!("nested sensor must be scoped");
		};
		let refs = &clone.scripts[0].settings.references;
		assert_eq!(
			refs["sensor"],
			ScriptReference::Entity {
				entity: mapping[sensor].clone()
			}
		);
		assert_eq!(
			refs["out"],
			ScriptReference::Entity {
				entity: key("scene-outside")
			}
		);
	}
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
	let document = session.authored_document();
	let member = document
		.entities
		.iter()
		.find(|entity| entity.name == "grate")
		.unwrap();
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command: EditorCommandDto::Rename {
				key: member.key.as_str().into(),
				name: "Locally labeled grate".into(),
			},
		})
		.unwrap();
	let authored = session.authored_document();
	let reopened = scene_data::parse_json(&authored.to_json().unwrap())
		.unwrap()
		.data;
	assert_eq!(authored.as_ref(), &reopened);
	let mut previous = reopened.clone();
	let folder = scene_data::FolderId::new("props").unwrap();
	previous
		.organization
		.folders
		.push(scene_data::FolderRecord {
			id: folder.clone(),
			name: "Props".into(),
			parent: None,
		});
	let anchor = previous
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap();
	previous
		.organization
		.root_membership
		.insert(anchor.key.clone(), folder);
	assert!(anchor.linked_prefab.as_ref().unwrap().overrides.len() == 1);
	let mut previous_wire: serde_json::Value =
		serde_json::from_str(&previous.to_json().unwrap()).unwrap();
	previous_wire["schema_version"] = 6.into();
	let migrated = scene_data::parse_json(&previous_wire.to_string())
		.unwrap()
		.data;
	assert_eq!(migrated.organization, previous.organization);
	assert_eq!(
		&migrated.entities[..previous.entities.len()],
		previous.entities
	);
	assert!(migrated.default_game_camera.is_some());
	assert_eq!(
		scene_data::parse_json(&migrated.to_json().unwrap())
			.unwrap()
			.data,
		migrated
	);
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
		.filter(|(entity, _)| matches!(entity.name.as_str(), "grate" | "Locally labeled grate"))
		.map(|(_, matrix)| matrix.w_axis.x)
		.collect();
	grates.sort_by(f32::total_cmp);
	assert_eq!(grates, vec![5.0, 7.0, 21.0, 23.0]);
}

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

fn assembly() -> LinkedPrefabSource {
	let mut floor = source("floor", "a");
	floor
		.prefab
		.reference_slots
		.insert("Actor".into(), key("external-actor"));
	let mut grate = source("grate", "b");
	grate
		.prefab
		.reference_slots
		.insert("Guide".into(), key("external-guide"));
	grate.prefab.entities[0]
		.native_behaviors
		.push(NativeBehaviorComponent {
			id: ComponentId::new("spawner").unwrap(),
			enabled: true,
			behavior: NativeBehaviorSpec::TimedSpawner {
				prefab_path: "prefabs/bolt.json".into(),
				interval_ticks: 10,
				max_alive: 1,
				lifetime_ticks: 20,
				spawn_offset: [0.0; 3],
				bindings: BTreeMap::from([
					("local".into(), key("grate")),
					("guide".into(), key("external-guide")),
				]),
			},
		});
	let bolt = source("bolt", "c");
	let bolt_version = PinnedAssetVersion {
		asset_id: bolt.source_id.clone(),
		fingerprint: bolt.fingerprint.clone(),
	};
	grate.prefab = grate
		.prefab
		.with_nested_placement(
			&grate.source_id,
			&bolt,
			placement("bolt", &bolt_version, "grate", 0.5),
		)
		.unwrap();
	let grate_version = PinnedAssetVersion {
		asset_id: grate.source_id.clone(),
		fingerprint: grate.fingerprint.clone(),
	};
	for (id, x, binding) in [
		("left", 2.0, NestedBinding::Object { key: key("floor") }),
		(
			"right",
			4.0,
			NestedBinding::Slot {
				name: "Actor".into(),
			},
		),
	] {
		let mut child = placement(id, &grate_version, "floor", x);
		child.bindings.insert("Guide".into(), binding);
		floor.prefab = floor
			.prefab
			.with_nested_placement(&floor.source_id, &grate, child)
			.unwrap();
	}
	floor
}

fn assembly_edit(session: &mut EditorSession, command: EditorCommandDto) -> Result<(), String> {
	session
		.execute_command(EditorCommandRequest {
			expected_revision: session.revision(),
			command,
		})
		.map(|_| ())
}

fn instantiate_assembly(source: LinkedPrefabSource) -> (EditorSession, EntityKey) {
	let mut session = EditorSession::new(scene(vec![entity("actor", [0.0; 3])])).unwrap();
	assembly_edit(
		&mut session,
		EditorCommandDto::InstantiateLinkedPrefab {
			source,
			position: [10.0, 0.0, 0.0],
			bindings: BTreeMap::from([("Actor".into(), key("actor"))]),
		},
	)
	.unwrap();
	let anchor = session
		.authored_document()
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap()
		.key
		.clone();
	(session, anchor)
}

fn placement_member(
	link: &LinkedPrefabInstance,
	placement: &str,
	entity: Option<&str>,
) -> EntityKey {
	let placement_path = vec![PlacementId::new(placement).unwrap()];
	let address = match entity {
		Some(entity) => PropAddress::Object {
			placement_path,
			source_entity: key(entity),
		},
		None => PropAddress::Placement { placement_path },
	};
	link.entity_map[&address.materialized_key().unwrap()].clone()
}

#[test]
fn duplicate_nested_assembly_remaps_internal_and_preserves_external_references() {
	let (mut session, root) = instantiate_assembly(assembly());
	let initial = session.authored_document();
	let original = initial
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	let left = placement_member(original, "left", Some("grate"));
	assembly_edit(
		&mut session,
		EditorCommandDto::Rename {
			key: left.as_str().into(),
			name: "Left override".into(),
		},
	)
	.unwrap();
	let before_duplicate = session.authored_document();
	assembly_edit(
		&mut session,
		EditorCommandDto::DuplicateLinkedPrefab {
			root_key: root.as_str().into(),
		},
	)
	.unwrap();
	let copy = session.authored_document();
	let duplicate = copy
		.entities
		.iter()
		.find(|entity| entity.key != root && entity.linked_prefab.is_some())
		.and_then(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	assert_eq!(duplicate.bindings["Actor"], key("actor"));
	assert_eq!(duplicate.source, original.source);
	assert!(
		duplicate
			.entity_map
			.iter()
			.all(|(source, member)| member != &original.entity_map[source])
	);
	let left_copy = placement_member(duplicate, "left", Some("grate"));
	let right_copy = placement_member(duplicate, "right", Some("grate"));
	for (key, target) in [
		(&left_copy, &duplicate.entity_map[&key("floor")]),
		(&right_copy, &key("actor")),
	] {
		let member = copy
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.unwrap();
		let NativeBehaviorSpec::TimedSpawner { bindings, .. } =
			&member.native_behaviors[0].behavior
		else {
			panic!("Expected copied spawner");
		};
		assert_eq!(&bindings["local"], key);
		assert_eq!(&bindings["guide"], target);
	}
	assert_eq!(duplicate.overrides.len(), 1);
	assert_eq!(
		copy.entities
			.iter()
			.find(|entity| entity.key == left_copy)
			.unwrap()
			.name,
		"Left override"
	);
	assembly_edit(
		&mut session,
		EditorCommandDto::Rename {
			key: left_copy.as_str().into(),
			name: "Edited duplicate".into(),
		},
	)
	.unwrap();
	assert_eq!(
		session
			.authored_document()
			.entities
			.iter()
			.find(|entity| entity.key == left)
			.unwrap()
			.name,
		"Left override"
	);
	assembly_edit(&mut session, EditorCommandDto::Undo).unwrap();
	assembly_edit(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), before_duplicate);
}

#[test]
fn unpack_outer_promotes_direct_placements_and_unpack_all_keeps_runtime_objects() {
	let (mut session, root) = instantiate_assembly(assembly());
	let initial = session.authored_document();
	let link = initial
		.entities
		.iter()
		.find_map(|entity| entity.linked_prefab.as_ref())
		.unwrap();
	let left_anchor = placement_member(link, "left", None);
	let right_anchor = placement_member(link, "right", None);
	let left_grate = placement_member(link, "left", Some("grate"));
	for (key, name) in [
		(&left_anchor, "Left placement"),
		(&left_grate, "Renamed grate"),
	] {
		assembly_edit(
			&mut session,
			EditorCommandDto::Rename {
				key: key.as_str().into(),
				name: name.into(),
			},
		)
		.unwrap();
	}
	let nested = session.authored_document();
	let runtime = resolve_scene(&nested);
	assembly_edit(
		&mut session,
		EditorCommandDto::UnpackLinkedPrefabOuter {
			root_key: root.as_str().into(),
		},
	)
	.unwrap();
	let partially_unpacked = session.authored_document();
	assert_eq!(resolve_scene(&partially_unpacked), runtime);
	let promoted: BTreeMap<_, _> = partially_unpacked
		.entities
		.iter()
		.filter_map(|entity| {
			entity
				.linked_prefab
				.as_ref()
				.map(|link| (&entity.key, link))
		})
		.collect();
	assert_eq!(promoted.len(), 2);
	let left = promoted[&left_anchor];
	assert_eq!(left.bindings["Guide"], link.entity_map[&key("floor")]);
	assert_eq!(promoted[&right_anchor].bindings["Guide"], key("actor"));
	assert_eq!(left.entity_map[&key("grate")], left_grate);
	assert_eq!(left.source.prefab.composition.placements.len(), 1);
	assert_eq!(left.source.prefab.composition.pins.len(), 1);
	assert_eq!(
		left.overrides.iter().next().unwrap().source_entity,
		key("grate")
	);
	assert_eq!(
		override_status(&partially_unpacked, &left_anchor).unwrap()[0].value,
		LinkedPrefabValue::Name("Renamed grate".into())
	);
	assembly_edit(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), nested);
	assembly_edit(
		&mut session,
		EditorCommandDto::UnpackLinkedPrefab {
			root_key: root.as_str().into(),
		},
	)
	.unwrap();
	assert_eq!(session.authored_document().as_ref(), &runtime);
	assembly_edit(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), nested);
}

#[test]
fn relink_rejects_incompatible_bytes_then_preserves_runtime_and_undoable_provenance() {
	let pinned = assembly();
	let (mut session, root) = instantiate_assembly(pinned.clone());
	let before = session.authored_document();
	let mut replacement = pinned.clone();
	replacement.source_id = asset_catalog::AssetId::generate();
	replacement.path = "prefabs/recovered-floor.json".into();
	replacement.fingerprint = "d".repeat(64);
	let mut incompatible = replacement.clone();
	incompatible.prefab.entities[0].transform.position[0] = 3.0;
	let revision = session.revision();
	assert!(
		assembly_edit(
			&mut session,
			EditorCommandDto::RelinkLinkedPrefab {
				root_key: root.as_str().into(),
				source: incompatible,
			}
		)
		.unwrap_err()
		.contains("replacement differs")
	);
	assert_eq!(session.revision(), revision);
	assert_eq!(session.authored_document(), before);
	let review = review_recovery_source(&before, &root, &replacement).unwrap();
	assert_eq!(review.prior_source_id, pinned.source_id);
	assert_eq!(review.source_id, replacement.source_id);
	assert_eq!(review.member_count, 9);
	assert_eq!(review.binding_count, 1);
	assembly_edit(
		&mut session,
		EditorCommandDto::RelinkLinkedPrefab {
			root_key: root.as_str().into(),
			source: replacement.clone(),
		},
	)
	.unwrap();
	let recovered = session.authored_document();
	assert_eq!(resolve_scene(&recovered), resolve_scene(&before));
	assert_eq!(
		recovered
			.entities
			.iter()
			.find_map(|entity| entity.linked_prefab.as_ref())
			.unwrap()
			.source,
		replacement
	);
	assembly_edit(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), before);
}

#[test]
fn refreshing_one_placement_only_releases_pins_unreachable_from_any_placement() {
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

#[test]
fn direct_placement_apply_stages_one_draft_and_rejects_wrong_owners_or_stale_bases() {
	let floor = source("floor", "1");
	let grate = source("grate", "2");
	let child = PinnedAssetVersion {
		asset_id: grate.source_id.clone(),
		fingerprint: grate.fingerprint.clone(),
	};
	let left = placement("left", &child, "floor", 1.0);
	let right = placement("right", &child, "floor", 2.0);
	let saved = floor
		.prefab
		.with_nested_placement(&floor.source_id, &grate, left.clone())
		.unwrap()
		.with_nested_placement(&floor.source_id, &grate, right.clone())
		.unwrap();
	let placement = PropAddress::Placement {
		placement_path: vec![left.id.clone()],
	};
	let before = LinkedPrefabValue::Vector(left.local.position);
	let edited = LinkedPrefabValue::Vector([4.0, 0.0, 0.0]);
	let staged = saved
		.with_applied_prop_override(
			&floor.source_id,
			&placement,
			&LinkedPrefabProperty::TransformPosition,
			&before,
			edited.clone(),
		)
		.unwrap();
	assert_eq!(
		staged.composition.placements[0].local.position,
		[4.0, 0.0, 0.0]
	);
	assert_eq!(staged.composition.placements[1], right);
	assert_eq!(staged.composition.pins, saved.composition.pins);
	assert_eq!(saved.composition.placements[0], left);
	assert!(
		staged
			.with_applied_prop_override(
				&floor.source_id,
				&placement,
				&LinkedPrefabProperty::TransformPosition,
				&before,
				edited,
			)
			.unwrap_err()
			.contains("changed this property")
	);

	let child_object = PropAddress::Object {
		placement_path: vec![left.id.clone()],
		source_entity: key("grate"),
	};
	assert!(
		saved
			.with_applied_prop_override(
				&floor.source_id,
				&child_object,
				&LinkedPrefabProperty::Name,
				&LinkedPrefabValue::Name("grate".into()),
				LinkedPrefabValue::Name("Second grate".into()),
			)
			.unwrap_err()
			.contains("does not own")
	);
	let root_object = PropAddress::Object {
		placement_path: vec![],
		source_entity: key("floor"),
	};
	let staged = staged
		.with_applied_prop_override(
			&floor.source_id,
			&root_object,
			&LinkedPrefabProperty::Name,
			&LinkedPrefabValue::Name("floor".into()),
			LinkedPrefabValue::Name("New floor".into()),
		)
		.unwrap();
	assert_eq!(staged.entities[0].name, "New floor");
}

#[test]
fn typed_nested_overrides_revert_and_resolve_conflicts_independently() {
	fn execute(session: &mut EditorSession, command: EditorCommandDto) -> Result<(), String> {
		session
			.execute_command(EditorCommandRequest {
				expected_revision: session.revision(),
				command,
			})
			.map(|_| ())
	}

	let mut floor = source("floor", "1");
	let grate = source("grate", "2");
	let target = PinnedAssetVersion {
		asset_id: grate.source_id.clone(),
		fingerprint: grate.fingerprint.clone(),
	};
	let left = placement("left", &target, "floor", 1.0);
	let right = placement("right", &target, "floor", 2.0);
	floor.prefab = floor
		.prefab
		.with_nested_placement(&floor.source_id, &grate, left.clone())
		.unwrap()
		.with_nested_placement(&floor.source_id, &grate, right.clone())
		.unwrap();
	let left_address = PropAddress::Placement {
		placement_path: vec![left.id],
	};
	let right_address = PropAddress::Placement {
		placement_path: vec![right.id],
	};
	let mut session = EditorSession::new(scene(Vec::new())).unwrap();
	execute(
		&mut session,
		EditorCommandDto::InstantiateLinkedPrefab {
			source: floor.clone(),
			position: [0.0; 3],
			bindings: BTreeMap::new(),
		},
	)
	.unwrap();
	let original = session.authored_document();
	let anchor = original
		.entities
		.iter()
		.find(|entity| entity.linked_prefab.is_some())
		.unwrap();
	let root = anchor.key.clone();
	let map = &anchor.linked_prefab.as_ref().unwrap().entity_map;
	let left_object = map[&left_address.materialized_key().unwrap()].clone();
	let right_object = map[&right_address.materialized_key().unwrap()].clone();
	execute(
		&mut session,
		EditorCommandDto::SetTransforms {
			edits: [(left_object.clone(), 7.0), (right_object.clone(), 9.0)]
				.into_iter()
				.map(|(key, x)| TransformEditDto {
					key: key.as_str().into(),
					transform: TransformDto {
						position: [x, 0.0, 0.0],
						..Default::default()
					},
				})
				.collect(),
			space: TransformSpace::Local,
			snap: false,
		},
	)
	.unwrap();
	let local = session.authored_document();
	let rows = override_status(&local, &root).unwrap();
	assert_eq!(rows.len(), 2);
	assert!(rows.iter().all(|row| row.can_apply_to_root_source));
	let left_row = rows.iter().find(|row| row.address == left_address).unwrap();
	execute(
		&mut session,
		EditorCommandDto::RevertLinkedPrefab {
			root_key: root.as_str().into(),
			property: Some(left_row.target.clone()),
		},
	)
	.unwrap();
	let after_revert = override_status(&session.authored_document(), &root).unwrap();
	assert_eq!(after_revert.len(), 1);
	assert_eq!(after_revert[0].address, right_address);
	execute(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), local);

	let mut incoming = floor.clone();
	incoming.fingerprint = "3".repeat(64);
	incoming.prefab.composition.placements[0].name = "Grate at entrance".into();
	incoming.prefab.composition.placements[0].local.position[0] = 11.0;
	incoming.prefab.composition.placements[1].local.position[0] = 13.0;
	incoming.prefab.composition.placements.reverse();
	let reviewed = review_source_overrides(&local, &root, &incoming, None).unwrap();
	assert_eq!(reviewed.len(), 2);
	assert!(reviewed.iter().all(|row| row.conflict));
	let choices = vec![
		LinkedPrefabOverrideResolution {
			address: left_address.clone(),
			property: LinkedPrefabProperty::TransformPosition,
			resolution: PrefabConflictResolution::KeepLocal,
		},
		LinkedPrefabOverrideResolution {
			address: right_address.clone(),
			property: LinkedPrefabProperty::TransformPosition,
			resolution: PrefabConflictResolution::UseSource,
		},
	];
	let update = |resolutions| EditorCommandDto::UpdateLinkedPrefabReviewed {
		root_key: root.as_str().into(),
		source: incoming.clone(),
		bindings: None,
		resolutions,
	};
	let mut removed = incoming.clone();
	removed
		.prefab
		.composition
		.placements
		.retain(|entry| entry.id.as_str() != "left");
	let removed_reviews = review_source_overrides(&local, &root, &removed, None).unwrap();
	let unresolved = removed_reviews
		.iter()
		.find(|row| row.address == left_address)
		.unwrap();
	assert!(unresolved.conflict);
	assert_eq!(unresolved.incoming, None);
	assert!(
		execute(
			&mut session,
			EditorCommandDto::UpdateLinkedPrefabReviewed {
				root_key: root.as_str().into(),
				source: removed,
				bindings: None,
				resolutions: choices.clone(),
			}
		)
		.unwrap_err()
		.contains("Source removed overridden entity")
	);
	assert_eq!(session.authored_document(), local);
	let revision = session.revision();
	assert!(
		execute(&mut session, update(vec![choices[0].clone()]))
			.unwrap_err()
			.contains("conflicts")
	);
	assert_eq!(session.authored_document(), local);
	assert_eq!(session.revision(), revision);
	execute(&mut session, update(choices)).unwrap();
	let applied = session.authored_document();
	let x = |key: &EntityKey| {
		applied
			.entities
			.iter()
			.find(|entity| &entity.key == key)
			.unwrap()
			.transform
			.position[0]
	};
	assert_eq!(x(&left_object), 7.0);
	assert_eq!(x(&right_object), 13.0);
	let remaining = override_status(&applied, &root).unwrap();
	assert_eq!(remaining.len(), 1);
	assert_eq!(remaining[0].address, left_address);
	execute(&mut session, EditorCommandDto::Undo).unwrap();
	assert_eq!(session.authored_document(), local);
}
