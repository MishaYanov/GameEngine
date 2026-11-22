use super::*;

pub(super) fn fresh_key(data: &SceneData) -> EntityKey {
	loop {
		let key = EntityKey::generate();
		if !data.entities.iter().any(|entity| entity.key == key)
			&& !data.lights.iter().any(|light| light.key == key)
		{
			return key;
		}
	}
}

pub(super) fn duplicate(
	data: &mut SceneData,
	selection: &mut Vec<EntityKey>,
	offset: Vec3,
) -> Result<(), String> {
	let hierarchy = hierarchy(data)?;
	let roots = selection_roots(&hierarchy, selection)?;
	let indices = selected_subtree(&hierarchy, &roots);
	let roots: HashSet<_> = roots.into_iter().collect();
	if data
		.entities
		.len()
		.checked_add(indices.len())
		.is_none_or(|count| count > MAX_SCENE_ENTITIES)
	{
		return Err("Duplicating this selection would exceed the scene entity limit".into());
	}
	let original_world = data.world_matrices()?;
	let mut keys = HashMap::with_capacity(indices.len());
	let mut used: HashSet<_> = data
		.entities
		.iter()
		.map(|entity| entity.key.clone())
		.chain(data.lights.iter().map(|light| light.key.clone()))
		.collect();
	for &index in &indices {
		let key = loop {
			let key = EntityKey::generate();
			if used.insert(key.clone()) {
				break key;
			}
		};
		keys.insert(data.entities[index].key.clone(), key);
	}
	let mut copies = Vec::with_capacity(indices.len());
	let linked_members: HashSet<_> = data
		.entities
		.iter()
		.filter_map(|entity| entity.linked_prefab.as_ref())
		.flat_map(|link| link.entity_map.values().cloned())
		.collect();
	for index in indices {
		let mut copy = data.entities[index].clone();
		copy.clear_local_player();
		if !linked_members.contains(&copy.key) {
			copy.name.push_str(" Copy");
		}
		copy.key = keys[&copy.key].clone();
		copy.parent = copy
			.parent
			.map(|key| keys.get(&key).cloned().unwrap_or(key));
		copy.remap_component_references(&keys)?;
		if let Some(link) = &mut copy.linked_prefab {
			link.remap_instance(&keys)?;
		}
		if roots.contains(&index) && offset != Vec3::ZERO {
			let target = original_world[index].w_axis.truncate() + offset;
			let position = match hierarchy.parents[index] {
				Some(parent) => inverse(original_world[parent])?.transform_point3(target),
				None => target,
			};
			copy.transform.position = finite_result(position, "Duplicated position")?.to_array();
		}
		copies.push(copy);
	}
	*selection = selection.iter().map(|key| keys[key].clone()).collect();
	let folder_copies = keys
		.iter()
		.filter_map(|(original, copy)| {
			data.organization
				.root_membership
				.get(original)
				.map(|folder| (copy.clone(), folder.clone()))
		})
		.collect::<Vec<_>>();
	data.organization.root_membership.extend(folder_copies);
	// Existing semantic role bindings continue to identify the originals.
	data.entities.extend(copies);
	Ok(())
}
