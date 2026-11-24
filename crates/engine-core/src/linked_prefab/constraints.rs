use super::*;

pub(crate) fn is_transform_only_anchor(entity: &EntityData) -> bool {
	!entity.has_mesh_component()
		&& entity.collider.is_none()
		&& entity.native_behaviors.is_empty()
		&& entity.scripts.is_empty()
		&& entity.character_controller.is_none()
		&& entity.animation.is_none()
		&& entity.animator.is_none()
		&& entity.interactable.is_none()
		&& entity.camera.is_none()
		&& entity.player_controller.is_none()
		&& entity.audio_source.is_none()
		&& entity.sky.is_none()
		&& entity.particle_emitter.is_none()
		&& entity.light.is_none()
}

pub fn validate_scene_links(scene: &SceneData) -> Result<(), String> {
	let entities: HashMap<_, _> = scene
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let mut children: HashMap<&EntityKey, Vec<&EntityKey>> = HashMap::new();
	for entity in &scene.entities {
		if let Some(parent) = &entity.parent {
			children.entry(parent).or_default().push(&entity.key);
		}
	}
	let mut members = HashSet::new();
	let mut count = 0usize;
	for anchor in scene
		.entities
		.iter()
		.filter(|entity| entity.linked_prefab.is_some())
	{
		count += 1;
		if count > 128 {
			return Err("Scene exceeds 128 linked prefab instances".into());
		}
		let link = anchor.linked_prefab.as_ref().expect("filtered link");
		link.source.validate()?;
		if !is_transform_only_anchor(anchor) {
			return Err(
				"Linked instance anchors are transform-only objects; edit a member or Unpack first"
					.into(),
			);
		}
		let expanded = link.source.prefab.expanded_entities()?;
		let sources: HashMap<_, _> = expanded
			.iter()
			.map(|entity| (&entity.key, entity))
			.collect();
		let source_keys: HashSet<_> = sources.keys().copied().collect();
		if source_keys.len() != link.entity_map.len()
			|| link.entity_map.keys().any(|key| !source_keys.contains(key))
		{
			return Err("Linked instance mapping differs from its pinned source".into());
		}
		let owned: HashSet<_> = link.entity_map.values().collect();
		let mut pending = children.get(&anchor.key).cloned().unwrap_or_default();
		while let Some(key) = pending.pop() {
			if !owned.contains(key) {
				return Err("Linked instance structure is pinned; Unpack before adding or duplicating individual members".into());
			}
			if let Some(nested) = children.get(key) {
				pending.extend(nested);
			}
		}
		for (source, key) in &link.entity_map {
			if key == &anchor.key || !members.insert(key.clone()) {
				return Err(
					"Linked instance members must be distinct and belong to one instance".into(),
				);
			}
			if link.leaf_removed(source) {
				if entities.contains_key(key)
					|| scene.lights.iter().any(|light| &light.key == key)
					|| !link.supports_leaf_removal(source, &expanded)
				{
					return Err(
						"A removed Prop object must be an inherited leaf with a reserved scene key"
							.into(),
					);
				}
				continue;
			}
			let member = entities.get(key).ok_or(
				"A linked member was removed; delete the whole instance or Unpack it first",
			)?;
			let base = sources.get(source).expect("source mapping checked");
			let parent = base
				.parent
				.as_ref()
				.and_then(|key| link.entity_map.get(key))
				.unwrap_or(&anchor.key);
			if member.parent.as_ref() != Some(parent) || member.linked_prefab.is_some() {
				return Err(
					"Linked member hierarchy is pinned; Unpack before reparenting or nesting it"
						.into(),
				);
			}
		}
		for target in &link.overrides {
			if !source_keys.contains(&target.source_entity) {
				return Err("Override targets an entity outside its pinned source".into());
			}
			if target.property != LinkedPrefabProperty::Presence
				&& link.leaf_removed(&target.source_entity)
			{
				return Err("A removed Prop object cannot have other local overrides".into());
			}
		}
		for target in link.bindings.values() {
			if !entities.contains_key(target) {
				return Err(
					"Linked prefab reference binding targets a missing scene entity".into(),
				);
			}
		}
		let base = materialized_from(link, &expanded, &anchor.key)?;
		for (source, expected) in &base {
			if link.leaf_removed(source) {
				continue;
			}
			let actual = entities[&link.entity_map[source]];
			if actual.active != expected.active {
				return Err("Linked member activation belongs to the Prop source".into());
			}
			if actual.camera != expected.camera {
				return Err(
					"Linked Camera is owned by the Prop source; edit the source or Unpack first"
						.into(),
				);
			}
			if actual.player_controller != expected.player_controller {
				return Err("Linked Player Controller is owned by the Prop source; use a separate scene-root actor".into());
			}
			if actual.animator != expected.animator {
				return Err(
					"Linked Animator belongs to its source; open the Prop source or Unpack".into(),
				);
			}
			if actual.audio_source != expected.audio_source {
				return Err("Linked Audio Source is owned by the Prop source; edit the source or Unpack first".into());
			}
			if actual.sky != expected.sky {
				return Err(
					"Linked Sky is owned by the Prop source; edit the source or Unpack first"
						.into(),
				);
			}
			if actual.interactable != expected.interactable {
				return Err("Linked Interactable is owned by the prefab source; edit the source or Unpack first".into());
			}
			for property in properties(&[expected, actual]) {
				let target = LinkedPrefabOverrideTarget {
					source_entity: source.clone(),
					property: property.clone(),
				};
				if value(expected, &property) != value(actual, &property)
					&& !link.overrides.contains(&target)
				{
					return Err(
						"Linked property differs from its pin without an explicit override".into(),
					);
				}
			}
		}
	}
	Ok(())
}
