use super::*;
use crate::{
	components::{ANIMATION_TYPE, MESH_TYPE},
	editor::PrefabData,
	project::ProjectData,
	scene_data::{EntityData, SceneData},
};
use std::collections::{HashMap, HashSet};

impl AuthoredClip {
	pub fn resolve_scene(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
		scene: &SceneData,
	) -> Result<ResolvedAuthoredClip, String> {
		self.validate()?;
		if !matches!(&self.owner, AuthoredClipOwner::Scene { scene_id: expected } if expected == scene_id)
		{
			return Err("Authored clip belongs to another Scene".into());
		}
		self.resolve(
			&scene.entities,
			protected_scene(project, scene_id, scene),
			|target| match target {
				VisualTargetId::Scene { entity_key } => Ok(entity_key.clone()),
				_ => Err("A Scene clip requires Scene object identities".into()),
			},
		)
	}

	pub fn resolve_prop(
		&self,
		owner: &AssetId,
		prefab: &PrefabData,
	) -> Result<ResolvedAuthoredClip, String> {
		self.validate()?;
		self.check_prop_owner(owner)?;
		if prefab.gameplay.is_some() {
			return Err("Use a component Prop source for a portable authored clip".into());
		}
		let projection = prefab.expanded_composition(Some(owner))?;
		let objects = projection
			.addresses
			.into_iter()
			.map(|(key, address)| (address, key))
			.collect::<BTreeMap<_, _>>();
		self.resolve(&projection.entities, HashSet::new(), |target| {
			Ok(prop_object(&objects, target)?.clone())
		})
	}

	/// Resolve the same portable source address in each pinned instance independently.
	pub fn resolve_prop_instance(
		&self,
		project: &ProjectData,
		scene_id: &SceneId,
		scene: &SceneData,
		root: &EntityKey,
	) -> Result<ResolvedAuthoredClip, String> {
		self.validate()?;
		let link = scene
			.entities
			.iter()
			.find(|entity| &entity.key == root)
			.and_then(|entity| entity.linked_prefab.as_ref())
			.ok_or("Authored clip requires a linked Prop instance")?;
		self.check_prop_owner(&link.source.source_id)?;
		let projection = link
			.source
			.prefab
			.expanded_composition(Some(&link.source.source_id))?;
		let objects = projection
			.addresses
			.into_iter()
			.map(|(key, address)| (address, key))
			.collect::<BTreeMap<_, _>>();
		self.resolve(
			&scene.entities,
			protected_scene(project, scene_id, scene),
			|target| {
				link.entity_map
					.get(prop_object(&objects, target)?)
					.cloned()
					.ok_or_else(|| {
						"Authored clip object is missing from this pinned Prop instance".into()
					})
			},
		)
	}

	fn check_prop_owner(&self, owner: &AssetId) -> Result<(), String> {
		if matches!(&self.owner, AuthoredClipOwner::Prop { asset_id } if asset_id == owner) {
			Ok(())
		} else {
			Err("Authored clip belongs to another Prop asset".into())
		}
	}

	fn resolve(
		&self,
		entities: &[EntityData],
		protected: HashSet<EntityKey>,
		resolve_key: impl Fn(&VisualTargetId) -> Result<EntityKey, String>,
	) -> Result<ResolvedAuthoredClip, String> {
		let by_key: HashMap<_, _> = entities
			.iter()
			.map(|entity| (&entity.key, entity))
			.collect();
		let blocked = blocked_subtrees(entities, &by_key, protected);
		let bindings = self
			.tracks
			.iter()
			.map(|track| &track.target)
			.collect::<BTreeSet<_>>()
			.into_iter()
			.map(|target| {
				let key = resolve_key(target)?;
				let entity = by_key
					.get(&key)
					.ok_or_else(|| format!("Authored clip object {} is missing", key.as_str()))?;
				if entity.parent.is_none()
					|| !entity.has_mesh_component()
					|| !entity.renderable
					|| blocked.contains(&key)
				{
					return Err(format!(
						"Authored clip target {} must be a visible child with a visual-only subtree",
						key.as_str()
					));
				}
				Ok((target.clone(), VisualPose::bind(key, &entity.transform)))
			})
			.collect::<Result<_, String>>()?;
		Ok(ResolvedAuthoredClip {
			clip: self.clone(),
			bindings,
		})
	}
}

fn protected_scene(
	project: &ProjectData,
	scene_id: &SceneId,
	scene: &SceneData,
) -> HashSet<EntityKey> {
	scene
		.bindings
		.values()
		.cloned()
		.chain(
			project
				.behaviors_for(scene_id)
				.map(|behavior| behavior.entity().clone()),
		)
		.collect()
}

fn blocked_subtrees(
	entities: &[EntityData],
	by_key: &HashMap<&EntityKey, &EntityData>,
	mut blocked: HashSet<EntityKey>,
) -> HashSet<EntityKey> {
	for entity in entities {
		if entity
			.component_records()
			.iter()
			.any(|component| !matches!(component.type_id(), MESH_TYPE | ANIMATION_TYPE))
		{
			blocked.insert(entity.key.clone());
		}
		for behavior in &entity.native_behaviors {
			blocked.extend(behavior.behavior.entity_references().into_iter().cloned());
		}
	}
	let mut queue: Vec<_> = blocked.iter().cloned().collect();
	while let Some(key) = queue.pop() {
		if let Some(parent) = by_key.get(&key).and_then(|entity| entity.parent.as_ref()) {
			if blocked.insert(parent.clone()) {
				queue.push(parent.clone());
			}
		}
	}
	blocked
}

fn prop_object<'a>(
	objects: &'a BTreeMap<PropAddress, EntityKey>,
	target: &VisualTargetId,
) -> Result<&'a EntityKey, String> {
	let VisualTargetId::Prop { address } = target else {
		return Err("A Prop clip requires source-local addresses".into());
	};
	objects
		.get(address)
		.ok_or_else(|| format!("Authored clip address {address:?} is missing"))
}
