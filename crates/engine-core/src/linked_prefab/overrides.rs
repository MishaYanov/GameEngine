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
		for component in &entity.native_behaviors {
			properties.insert(NativeBehavior {
				component_id: component.id.clone(),
			});
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
					NativeBehavior { component_id } => {
						component.type_id() == crate::components::NATIVE_TYPE
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
	let base = materialized(link, anchor)?;
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
			Ok(LinkedPrefabOverrideDto {
				target: target.clone(),
				base: value(expected, &target.property),
				value: value(actual, &target.property),
			})
		})
		.collect()
}
