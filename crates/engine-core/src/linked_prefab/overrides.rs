use super::*;

pub(crate) fn properties(entities: &[&EntityData]) -> BTreeSet<LinkedPrefabProperty> {
	use LinkedPrefabProperty::*;
	let mut properties = BTreeSet::from([
		Name,
		TransformPosition,
		TransformRotation,
		TransformScale,
		MeshRenderer,
		Collider,
		CharacterController,
		Animation,
	]);
	for entity in entities {
		if entity.particle_emitter.is_some() {
			properties.insert(ParticleEmitter);
		}
		for component in &entity.native_behaviors {
			properties.insert(NativeBehavior {
				component_id: component.id.clone(),
			});
		}
		for component in &entity.scripts {
			properties.insert(Script { component_id: component.id.clone() });
		}
	}
	properties
}

pub(crate) fn value(entity: &EntityData, property: &LinkedPrefabProperty) -> LinkedPrefabValue {
	use LinkedPrefabProperty::*;
	match property {
		CharacterController => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|r| r.type_id() == crate::components::CHARACTER_TYPE),
		),
		Animation => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|r| r.type_id() == crate::components::ANIMATION_TYPE),
		),
		ParticleEmitter => LinkedPrefabValue::Component(
			entity.component_records().into_iter()
				.find(|record| record.type_id() == crate::components::PARTICLE_EMITTER_TYPE),
		),
		Name => LinkedPrefabValue::Name(entity.name.clone()),
		TransformPosition => LinkedPrefabValue::Vector(entity.transform.position),
		TransformRotation => LinkedPrefabValue::Vector(entity.transform.rotation),
		TransformScale => LinkedPrefabValue::Vector(entity.transform.scale),
		MeshRenderer => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|record| record.type_id() == crate::components::MESH_TYPE),
		),
		Collider => LinkedPrefabValue::Component(
			entity
				.component_records()
				.into_iter()
				.find(|record| record.type_id() == crate::components::COLLIDER_TYPE),
		),
		NativeBehavior { component_id } => {
			LinkedPrefabValue::Component(entity.component_records().into_iter().find(|record| {
				record.type_id() == crate::components::NATIVE_TYPE && &record.id == component_id
			}))
		}
		Script { component_id } => {
			LinkedPrefabValue::Component(entity.component_records().into_iter().find(|record| {
				record.type_id() == crate::components::SCRIPT_TYPE && &record.id == component_id
			}))
		}
	}
}

pub(crate) fn apply_value(
	entity: &mut EntityData,
	property: &LinkedPrefabProperty,
	new: LinkedPrefabValue,
) -> Result<(), String> {
	use LinkedPrefabProperty::*;
	match (property, new) {
		(Name, LinkedPrefabValue::Name(name)) => entity.name = name,
		(TransformPosition, LinkedPrefabValue::Vector(vector)) => {
			entity.transform.position = vector
		}
		(TransformRotation, LinkedPrefabValue::Vector(vector)) => {
			entity.transform.rotation = vector
		}
		(TransformScale, LinkedPrefabValue::Vector(vector)) => entity.transform.scale = vector,
		(property, LinkedPrefabValue::Component(component)) => {
			let current = match value(entity, property) {
				LinkedPrefabValue::Component(component) => component,
				_ => return Err("Override property/value types differ".into()),
			};
			if let Some(current) = current {
				entity.remove_component(&current.id)?;
			}
			if let Some(component) = component {
				let correct = match property {
					CharacterController => component.type_id() == crate::components::CHARACTER_TYPE,
					Animation => component.type_id() == crate::components::ANIMATION_TYPE,
					MeshRenderer => component.type_id() == crate::components::MESH_TYPE,
					Collider => component.type_id() == crate::components::COLLIDER_TYPE,
					ParticleEmitter => component.type_id() == crate::components::PARTICLE_EMITTER_TYPE,
					NativeBehavior { component_id } => {
						component.type_id() == crate::components::NATIVE_TYPE
							&& &component.id == component_id
					}
					Script { component_id } => {
						component.type_id() == crate::components::SCRIPT_TYPE
							&& &component.id == component_id
					}
					_ => false,
				};
				if !correct {
					return Err(
						"Override component type or identity differs from its property address"
							.into(),
					);
				}
				entity.put_component(component, false)?;
			}
		}
		_ => return Err("Override property/value types differ".into()),
	}
	Ok(())
}

pub(crate) fn supports_source_override(
	address: &PropAddress,
	property: &LinkedPrefabProperty,
) -> bool {
	use LinkedPrefabProperty::*;
	match address {
		PropAddress::Object { placement_path, .. } => {
			placement_path.is_empty() && !matches!(property, NativeBehavior { .. } | Script { .. })
		}
		PropAddress::Placement { placement_path } => {
			placement_path.len() == 1
				&& matches!(
					property,
					TransformPosition | TransformRotation | TransformScale
				)
		}
	}
}

/// Normal Inspector commands record user intent even when its new value equals
/// a future source value. Only explicit Revert clears that persisted intention.
pub fn capture_overrides(before: &SceneData, after: &mut SceneData) -> Result<(), String> {
	let previous: HashMap<_, _> = before
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let indices: HashMap<_, _> = after
		.entities
		.iter()
		.enumerate()
		.map(|(index, entity)| (entity.key.clone(), index))
		.collect();
	let anchors: Vec<_> = after
		.entities
		.iter()
		.enumerate()
		.filter_map(|(index, entity)| {
			entity
				.linked_prefab
				.as_ref()
				.map(|link| (index, entity.key.clone(), link.clone()))
		})
		.collect();
	for (anchor_index, anchor, mut link) in anchors {
		let Some(old) = previous
			.get(&anchor)
			.and_then(|entity| entity.linked_prefab.as_ref())
		else {
			continue;
		};
		// Explicit link commands own changes to their pin/masks as one history edit.
		if old != &link {
			continue;
		}
		for (source, target) in &link.entity_map {
			let (Some(before), Some(&index)) = (previous.get(target), indices.get(target)) else {
				continue;
			};
			let current = &after.entities[index];
			for property in properties(&[before, current]) {
				if value(before, &property) != value(current, &property) {
					link.overrides.insert(LinkedPrefabOverrideTarget {
						source_entity: source.clone(),
						property,
					});
				}
			}
		}
		after.entities[anchor_index].linked_prefab = Some(link);
	}
	Ok(())
}

pub fn override_status(
	scene: &SceneData,
	anchor: &EntityKey,
) -> Result<Vec<LinkedPrefabOverrideDto>, String> {
	let link = scene
		.entities
		.iter()
		.find(|entity| &entity.key == anchor)
		.and_then(|entity| entity.linked_prefab.as_ref())
		.ok_or("Select a linked instance")?;
	let projection = link
		.source
		.prefab
		.expanded_composition(Some(&link.source.source_id))?;
	let base = materialized_from(link, &projection.entities, anchor)?;
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	link.overrides
		.iter()
		.map(|target| {
			let expected = base
				.get(&target.source_entity)
				.ok_or("Override source entity is missing")?;
			let actual = entities
				.get(&link.entity_map[&target.source_entity])
				.ok_or("Linked member is missing")?;
			let address = projection
				.addresses
				.get(&target.source_entity)
				.ok_or("Override source address is missing")?;
			Ok(LinkedPrefabOverrideDto {
				target: target.clone(),
				address: address.clone(),
				can_apply_to_root_source: supports_source_override(address, &target.property),
				base: value(expected, &target.property),
				value: value(actual, &target.property),
			})
		})
		.collect()
}

/// Compare saved pin, current instance and proposed source as one read-only
/// operation. Incoming rows with missing targets require explicit removal.
pub fn review_source_overrides(
	scene: &SceneData,
	anchor: &EntityKey,
	incoming: &LinkedPrefabSource,
	bindings: Option<&BTreeMap<String, EntityKey>>,
) -> Result<Vec<LinkedPrefabOverrideReviewDto>, String> {
	Ok(prepare_linked_source_update(scene, anchor, incoming.clone(), bindings.cloned())?.reviews)
}

pub(crate) struct PreparedLinkedSourceUpdate {
	pub link: LinkedPrefabInstance,
	pub members: BTreeMap<EntityKey, EntityData>,
	pub prior_members: HashSet<EntityKey>,
	pub reviews: Vec<LinkedPrefabOverrideReviewDto>,
}

pub(crate) fn prepare_linked_source_update(
	scene: &SceneData,
	anchor: &EntityKey,
	source: LinkedPrefabSource,
	bindings: Option<BTreeMap<String, EntityKey>>,
) -> Result<PreparedLinkedSourceUpdate, String> {
	source.validate()?;
	let old = scene
		.entities
		.iter()
		.find(|entity| &entity.key == anchor)
		.and_then(|entity| entity.linked_prefab.as_ref())
		.ok_or("Select a linked instance")?;
	if source.source_id != old.source.source_id {
		return Err("Linked source identity changed; update requires the same asset".into());
	}
	let props = old
		.source
		.prefab
		.expanded_composition(Some(&old.source.source_id))?;
	let before = materialized_from(old, &props.entities, anchor)?;
	let actual: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let entity_map = source
		.prefab
		.expanded_entities()?
		.iter()
		.map(|entity| {
			let key = old
				.entity_map
				.get(&entity.key)
				.cloned()
				.unwrap_or_else(EntityKey::generate);
			(entity.key.clone(), key)
		})
		.collect();
	let bindings = bindings.unwrap_or_else(|| old.bindings.clone());
	if bindings.values().any(|entity| !actual.contains_key(entity)) {
		return Err("Linked prefab reference binding targets a missing scene entity".into());
	}
	let link = LinkedPrefabInstance {
		source,
		entity_map,
		bindings,
		overrides: old.overrides.clone(),
	};
	let members = materialized(&link, anchor)?;
	let reviews = old
		.overrides
		.iter()
		.map(|target| {
			let source_entity = &target.source_entity;
			let property = &target.property;
			let base = before
				.get(source_entity)
				.ok_or("Pinned override object is missing")?;
			let address = props
				.addresses
				.get(source_entity)
				.ok_or("Pinned override address is missing")?;
			let scene_key = old
				.entity_map
				.get(source_entity)
				.ok_or("Linked member map is missing")?;
			let local = actual.get(scene_key).ok_or("Linked member is missing")?;
			let base = value(base, property);
			let local = value(local, property);
			let incoming = members
				.get(source_entity)
				.map(|entity| value(entity, property));
			let conflict = incoming
				.as_ref()
				.is_none_or(|incoming| incoming != &base && incoming != &local);
			Ok(LinkedPrefabOverrideReviewDto {
				target: target.clone(),
				address: address.clone(),
				base,
				local,
				incoming,
				conflict,
			})
		})
		.collect::<Result<_, String>>()?;
	Ok(PreparedLinkedSourceUpdate {
		link,
		members,
		prior_members: old.entity_map.values().cloned().collect(),
		reviews,
	})
}
