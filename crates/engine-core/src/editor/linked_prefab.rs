//! Explicit linked-instance edits share the ordinary authoring history.
use super::EditorSession;
use crate::{
	EntityKey,
	components::NO_MESH_PATH,
	linked_prefab::{
		self, LinkedPrefabInstance, LinkedPrefabOverrideResolution, LinkedPrefabOverrideTarget,
		LinkedPrefabSource, PrefabConflictResolution,
	},
	scene_data::EntityData,
};
use std::collections::{BTreeMap, HashSet};

enum UpdateResolution {
	All(PrefabConflictResolution),
	Reviewed(Vec<LinkedPrefabOverrideResolution>),
}

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
		self.update_linked_prefab_with_resolution(
			root_key,
			source,
			bindings,
			UpdateResolution::All(resolution),
		)
	}

	pub fn update_linked_prefab_reviewed(
		&mut self,
		root_key: String,
		source: LinkedPrefabSource,
		bindings: Option<BTreeMap<String, EntityKey>>,
		resolutions: Vec<LinkedPrefabOverrideResolution>,
	) -> Result<(), String> {
		self.update_linked_prefab_with_resolution(
			root_key,
			source,
			bindings,
			UpdateResolution::Reviewed(resolutions),
		)
	}

	fn update_linked_prefab_with_resolution(
		&mut self,
		root_key: String,
		source: LinkedPrefabSource,
		bindings: Option<BTreeMap<String, EntityKey>>,
		resolution: UpdateResolution,
	) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		let root = EntityKey::new(root_key)?;
		let mut update =
			linked_prefab::prepare_linked_source_update(&self.data, &root, source, bindings)?;
		let mut choices = BTreeMap::new();
		let fallback = match resolution {
			UpdateResolution::All(resolution) => resolution,
			UpdateResolution::Reviewed(reviews) => {
				for review in reviews {
					if review.resolution == PrefabConflictResolution::Reject
						|| choices
							.insert((review.address, review.property), review.resolution)
							.is_some()
					{
						return Err(
							"Select one explicit resolution per reviewed Prop conflict".into()
						);
					}
				}
				PrefabConflictResolution::Reject
			}
		};
		let mut conflicts = Vec::new();
		for review in update.reviews {
			let target = &review.target;
			let resolution = if review.conflict {
				choices
					.remove(&(review.address.clone(), target.property.clone()))
					.unwrap_or(fallback)
			} else {
				PrefabConflictResolution::KeepLocal
			};
			if review.conflict && resolution == PrefabConflictResolution::UseSource {
				update.link.overrides.remove(target);
				continue;
			}
			if review.incoming.is_none() {
				return Err(format!(
					"Source removed overridden entity {:?}. Choose Use Source to remove it or Unpack to keep it",
					review.address,
				));
			}
			if review.conflict && resolution == PrefabConflictResolution::Reject {
				if conflicts.len() < 16 {
					conflicts.push(format!("{:?} {:?}", review.address, target.property));
				}
				continue;
			}
			let incoming = update
				.members
				.get_mut(&target.source_entity)
				.ok_or("Reviewed override member is missing")?;
			linked_prefab::apply_value(incoming, &target.property, review.local)?;
		}
		if !choices.is_empty() {
			return Err("A reviewed Prop conflict changed; review the source update again".into());
		}
		if !conflicts.is_empty() {
			return Err(format!(
				"Linked prefab update conflicts: {}. Choose Keep Local or Use Source explicitly",
				conflicts.join(", "),
			));
		}
		let mut next = (*self.data).clone();
		next.entities
			.retain(|entity| !update.prior_members.contains(&entity.key));
		next.entities.extend(update.members.into_values());
		let anchor = next
			.entities
			.iter_mut()
			.find(|entity| entity.key == root)
			.ok_or("Linked instance anchor is missing")?;
		anchor.linked_prefab = Some(update.link);
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
