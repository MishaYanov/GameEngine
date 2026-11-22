//! Explicit linked-instance edits share the ordinary authoring history.
use super::EditorSession;
use crate::{
	EntityKey,
	components::NO_MESH_PATH,
	linked_prefab::{
		self, LinkedPrefabInstance, LinkedPrefabOverrideTarget, LinkedPrefabSource,
		PrefabConflictResolution,
	},
	scene_data::EntityData,
};
use std::collections::{BTreeMap, HashMap, HashSet};

impl EditorSession {
	pub fn instantiate_linked_prefab(
		&mut self,
		source: LinkedPrefabSource,
		position: [f32; 3],
		bindings: BTreeMap<String, EntityKey>,
	) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		source.validate()?;
		if !position.iter().all(|value| value.is_finite()) {
			return Err("Linked instance placement must be finite".into());
		}
		let anchor = EntityKey::generate();
		let map = source
			.prefab
			.expanded_entities()?
			.iter()
			.map(|entity| (entity.key.clone(), EntityKey::generate()))
			.collect();
		let link = LinkedPrefabInstance {
			source,
			entity_map: map,
			bindings,
			overrides: Default::default(),
		};
		let members = linked_prefab::materialized(&link, &anchor)?;
		let mut group = EntityData::new(anchor.clone(), "Linked prefab instance", NO_MESH_PATH)
			.with_renderable(false);
		group.transform.position = position;
		group.linked_prefab = Some(link);
		let mut next = (*self.data).clone();
		next.entities.push(group);
		next.entities.extend(members.into_values());
		self.commit(next, vec![anchor], "Instantiate linked prefab")
	}

	pub fn revert_linked_prefab(
		&mut self,
		root_key: String,
		property: Option<LinkedPrefabOverrideTarget>,
	) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let root = EntityKey::new(root_key)?;
		let mut next = (*self.data).clone();
		let anchor = next
			.entities
			.iter()
			.position(|entity| entity.key == root)
			.ok_or("Linked instance anchor is missing")?;
		let mut link = next.entities[anchor]
			.linked_prefab
			.clone()
			.ok_or("Object is not a linked instance")?;
		let base = linked_prefab::materialized(&link, &root)?;
		if let Some(target) = property {
			if !link.overrides.remove(&target) {
				return Ok(());
			}
			let expected = base
				.get(&target.source_entity)
				.ok_or("Override source entity is missing")?;
			let actual = next
				.entities
				.iter_mut()
				.find(|entity| entity.key == link.entity_map[&target.source_entity])
				.ok_or("Linked member is missing")?;
			linked_prefab::apply_value(
				actual,
				&target.property,
				linked_prefab::value(expected, &target.property),
			)?;
		} else {
			for (source, expected) in base {
				let actual = next
					.entities
					.iter_mut()
					.find(|entity| entity.key == link.entity_map[&source])
					.ok_or("Linked member is missing")?;
				*actual = expected;
			}
			link.overrides.clear();
		}
		next.entities[anchor].linked_prefab = Some(link);
		self.commit(
			next,
			self.selection.clone(),
			"Revert linked prefab overrides",
		)
	}

	pub fn unpack_linked_prefab(&mut self, root_key: String) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let key = EntityKey::new(root_key)?;
		let mut next = (*self.data).clone();
		let anchor = next
			.entities
			.iter_mut()
			.find(|entity| entity.key == key)
			.ok_or("Linked instance anchor is missing")?;
		if anchor.linked_prefab.take().is_none() {
			return Err("Object is not a linked instance".into());
		}
		self.commit(next, self.selection.clone(), "Unpack linked prefab")
	}

	pub fn update_linked_prefab(
		&mut self,
		root_key: String,
		source: LinkedPrefabSource,
		resolution: PrefabConflictResolution,
		bindings: Option<BTreeMap<String, EntityKey>>,
	) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		source.validate()?;
		let root = EntityKey::new(root_key)?;
		let mut next = (*self.data).clone();
		let anchor_index = next
			.entities
			.iter()
			.position(|entity| entity.key == root)
			.ok_or("Linked instance anchor is missing")?;
		let old = next.entities[anchor_index]
			.linked_prefab
			.clone()
			.ok_or("Object is not a linked instance")?;
		if source.source_id != old.source.source_id {
			return Err(
				"Linked source identity changed; do not retarget an existing instance".into(),
			);
		}
		let old_base = linked_prefab::materialized(&old, &root)?;
		let old_members: HashMap<_, _> = next
			.entities
			.iter()
			.filter(|entity| old.entity_map.values().any(|key| key == &entity.key))
			.map(|entity| (entity.key.clone(), entity.clone()))
			.collect();
		let mut link = LinkedPrefabInstance {
			source,
			entity_map: BTreeMap::new(),
			bindings: bindings.unwrap_or_else(|| old.bindings.clone()),
			overrides: old.overrides.clone(),
		};
		for entity in link.source.prefab.expanded_entities()?.iter() {
			link.entity_map.insert(
				entity.key.clone(),
				old.entity_map
					.get(&entity.key)
					.cloned()
					.unwrap_or_else(EntityKey::generate),
			);
		}
		let mut fresh = linked_prefab::materialized(&link, &root)?;
		let mut conflicts = Vec::new();
		for target in &old.overrides {
			let Some(incoming) = fresh.get_mut(&target.source_entity) else {
				if resolution != PrefabConflictResolution::UseSource {
					return Err(format!(
						"Source removed overridden entity {}. Choose Use Source to remove it or Unpack to keep it",
						target.source_entity.as_str()
					));
				}
				link.overrides.remove(target);
				continue;
			};
			let before = old_base
				.get(&target.source_entity)
				.ok_or("Pinned source entity is missing")?;
			let local = old_members
				.get(&old.entity_map[&target.source_entity])
				.ok_or("Linked member is missing")?;
			let base_value = linked_prefab::value(before, &target.property);
			let local_value = linked_prefab::value(local, &target.property);
			let incoming_value = linked_prefab::value(incoming, &target.property);
			let conflict = incoming_value != base_value && incoming_value != local_value;
			if conflict && resolution == PrefabConflictResolution::Reject {
				conflicts.push(format!(
					"{} {:?}",
					target.source_entity.as_str(),
					target.property
				));
				continue;
			}
			if conflict && resolution == PrefabConflictResolution::UseSource {
				link.overrides.remove(target);
			} else {
				linked_prefab::apply_value(incoming, &target.property, local_value)?;
			}
		}
		if !conflicts.is_empty() {
			return Err(format!(
				"Linked prefab update conflicts: {}. Choose Keep Local or Use Source explicitly",
				conflicts
					.into_iter()
					.take(16)
					.collect::<Vec<_>>()
					.join(", ")
			));
		}
		let old_keys: HashSet<_> = old.entity_map.values().cloned().collect();
		next.entities
			.retain(|entity| !old_keys.contains(&entity.key));
		next.entities.extend(fresh.into_values());
		let anchor = next
			.entities
			.iter_mut()
			.find(|entity| entity.key == root)
			.expect("anchor retained");
		anchor.linked_prefab = Some(link);
		let available: HashSet<_> = next.entities.iter().map(|entity| &entity.key).collect();
		let selection = self
			.selection
			.iter()
			.filter(|key| available.contains(key))
			.cloned()
			.collect();
		self.commit(next, selection, "Update linked prefab")
	}
}
