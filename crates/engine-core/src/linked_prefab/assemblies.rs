//! Preserve materialized identities when splitting or recovering authoring links.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPrefabRecoveryReview {
	pub root_key: EntityKey,
	pub prior_source_id: AssetId,
	pub prior_path: String,
	pub prior_fingerprint: String,
	pub source_id: AssetId,
	pub source_path: String,
	pub source_fingerprint: String,
	pub member_count: usize,
	pub binding_count: usize,
	pub override_count: usize,
}

/// A recovery changes only the source provenance. Shape or payload differences
/// require an ordinary source update with a complete override review.
pub fn review_recovery_source(
	scene: &SceneData,
	root: &EntityKey,
	replacement: &LinkedPrefabSource,
) -> Result<LinkedPrefabRecoveryReview, String> {
	let instance = scene
		.entities
		.iter()
		.find(|entity| &entity.key == root)
		.and_then(|entity| entity.linked_prefab.as_ref())
		.ok_or("Select a linked Prop instance")?;
	replacement.validate()?;
	let before = &instance.source.prefab;
	let after = &replacement.prefab;
	if before.entities != after.entities
		|| before.reference_slots != after.reference_slots
		|| before.composition.placements != after.composition.placements
		|| !same_pin_payload(&before.composition, &after.composition)
	{
		return Err("The replacement differs from the pinned objects, reference slots, or nested versions. The existing instance is still available.".into());
	}
	Ok(LinkedPrefabRecoveryReview {
		root_key: root.clone(),
		prior_source_id: instance.source.source_id.clone(),
		prior_path: instance.source.path.clone(),
		prior_fingerprint: instance.source.fingerprint.clone(),
		source_id: replacement.source_id.clone(),
		source_path: replacement.path.clone(),
		source_fingerprint: replacement.fingerprint.clone(),
		member_count: instance.entity_map.len(),
		binding_count: instance.bindings.len(),
		override_count: instance.overrides.len(),
	})
}

pub(crate) fn independent_placements(
	outer: &LinkedPrefabInstance,
) -> Result<Vec<(EntityKey, LinkedPrefabInstance)>, String> {
	let composition = &outer.source.prefab.composition;
	let pins: HashMap<_, _> = composition
		.pins
		.iter()
		.map(|pin| (&pin.version, pin))
		.collect();
	let mut properties: HashMap<_, Vec<_>> = HashMap::new();
	for target in &outer.overrides {
		properties
			.entry(&target.source_entity)
			.or_default()
			.push(&target.property);
	}
	let mut links = Vec::with_capacity(composition.placements.len());
	for placement in &composition.placements {
		let pin = pins
			.get(&placement.target)
			.ok_or("The nested Prop pin is unavailable")?;
		let target = PropAddress::Placement {
			placement_path: vec![placement.id.clone()],
		}
		.materialized_key()?;
		let anchor = outer
			.entity_map
			.get(&target)
			.ok_or("The nested Prop placement is missing from the instance")?;
		let source = subassembly_source(pin, &pins)?;
		let scoped_keys = source
			.prefab
			.expanded_composition(Some(&source.source_id))?
			.addresses
			.into_iter()
			.map(|(key, address)| {
				let mut address = address;
				let path = match &mut address {
					PropAddress::Object { placement_path, .. }
					| PropAddress::Placement { placement_path } => placement_path,
				};
				path.insert(0, placement.id.clone());
				Ok((address.materialized_key()?, key))
			})
			.collect::<Result<BTreeMap<_, _>, String>>()?;
		let entity_map = scoped_keys
			.iter()
			.map(|(outer_key, key)| {
				outer
					.entity_map
					.get(outer_key)
					.cloned()
					.map(|target| (key.clone(), target))
					.ok_or_else(|| "The nested Prop member is missing from the instance".to_owned())
			})
			.collect::<Result<_, _>>()?;
		let bindings = placement
			.bindings
			.iter()
			.map(|(name, binding)| {
				let value = match binding {
					NestedBinding::Object { key } => outer.entity_map.get(key),
					NestedBinding::Slot { name } => outer.bindings.get(name),
				}
				.ok_or("The nested Prop binding is missing from the instance")?;
				Ok((name.clone(), value.clone()))
			})
			.collect::<Result<_, String>>()?;
		let overrides = scoped_keys
			.iter()
			.flat_map(|(outer_key, local_key)| {
				properties
					.get(outer_key)
					.into_iter()
					.flatten()
					.map(move |&property| LinkedPrefabOverrideTarget {
						source_entity: local_key.clone(),
						property: property.clone(),
					})
			})
			.collect();
		links.push((
			anchor.clone(),
			LinkedPrefabInstance {
				source,
				entity_map,
				bindings,
				overrides,
			},
		));
	}
	Ok(links)
}

fn same_pin_payload(before: &PropComposition, after: &PropComposition) -> bool {
	let incoming: HashMap<_, _> = after.pins.iter().map(|pin| (&pin.version, pin)).collect();
	before.pins.len() == incoming.len()
		&& before.pins.iter().all(|old| {
			incoming.get(&old.version).is_some_and(|new| {
				old.entities == new.entities
					&& old.reference_slots == new.reference_slots
					&& old.placements == new.placements
			})
		})
}

fn subassembly_source(
	root: &PinnedPropSource,
	pins: &HashMap<&PinnedAssetVersion, &PinnedPropSource>,
) -> Result<LinkedPrefabSource, String> {
	let mut reachable = BTreeMap::new();
	let mut remaining: Vec<_> = root
		.placements
		.iter()
		.map(|placement| &placement.target)
		.collect();
	while let Some(version) = remaining.pop() {
		if let std::collections::btree_map::Entry::Vacant(entry) = reachable.entry(version) {
			let pin = pins
				.get(version)
				.ok_or("The nested Prop pin is unavailable")?;
			entry.insert(*pin);
			remaining.extend(pin.placements.iter().map(|placement| &placement.target));
		}
	}
	Ok(LinkedPrefabSource {
		source_id: root.version.asset_id.clone(),
		path: root.path.clone(),
		fingerprint: root.version.fingerprint.clone(),
		prefab: PrefabData {
			schema_version: if root.placements.is_empty() { 3 } else { 4 },
			entities: root.entities.clone(),
			gameplay: None,
			reference_slots: root.reference_slots.clone(),
			composition: PropComposition {
				placements: root.placements.clone(),
				pins: reachable.into_values().cloned().collect(),
			},
		},
	})
}
