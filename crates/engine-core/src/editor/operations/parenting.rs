//! Parent or detach a set of scene objects in one authored operation.

use super::transform::{hierarchy, inverse, selection_roots};
use crate::{
	EntityKey,
	scene_data::{FolderId, SceneData, TransformData},
};
use std::collections::HashSet;

pub(super) fn reparent_roots(
	data: &mut SceneData,
	keys: &[EntityKey],
	parent: Option<&EntityKey>,
	keep_world: bool,
) -> Result<(), String> {
	let hierarchy = hierarchy(data)?;
	let roots = selection_roots(&hierarchy, keys)?;
	let parent_index = parent
		.map(|key| {
			hierarchy
				.indices
				.get(key.as_str())
				.copied()
				.ok_or_else(|| format!("Parent {:?} does not exist", key.as_str()))
		})
		.transpose()?;
	let selected: HashSet<_> = roots.iter().copied().collect();
	let mut ancestor = parent_index;
	while let Some(index) = ancestor {
		if selected.contains(&index) {
			return Err(
				"An object cannot be parented to itself or a selected object's descendant".into(),
			);
		}
		ancestor = hierarchy.parents[index];
	}
	let changed: Vec<_> = roots
		.into_iter()
		.filter(|&index| hierarchy.parents[index] != parent_index)
		.collect();
	if changed.is_empty() {
		return Ok(());
	}
	let transforms = if keep_world {
		let world = data.world_matrices()?;
		let parent_inverse = parent_index
			.map(|index| inverse(world[index]))
			.transpose()?;
		changed
			.iter()
			.map(|&index| {
				let local = parent_inverse.map_or(world[index], |matrix| matrix * world[index]);
				TransformData::from_matrix(local).map(Some)
			})
			.collect::<Result<Vec<_>, _>>()?
	} else {
		vec![None; changed.len()]
	};
	let folders = if parent_index.is_none() && !data.organization.root_membership.is_empty() {
		inherited_folders(data, &hierarchy.parents, &changed)
	} else {
		vec![None; changed.len()]
	};
	for ((index, transform), folder) in changed.into_iter().zip(transforms).zip(folders) {
		let entity = &mut data.entities[index];
		entity.parent = parent.cloned();
		if let Some(transform) = transform {
			entity.transform = transform;
		}
		data.organization.root_membership.remove(&entity.key);
		if let Some(folder) = folder {
			data.organization
				.root_membership
				.insert(entity.key.clone(), folder);
		}
	}
	Ok(())
}

fn inherited_folders(
	data: &SceneData,
	parents: &[Option<usize>],
	indices: &[usize],
) -> Vec<Option<FolderId>> {
	let mut root_indices = vec![None; parents.len()];
	let mut path = Vec::new();
	indices
		.iter()
		.map(|&index| {
			let mut node = index;
			let root = loop {
				if let Some(root) = root_indices[node] {
					break root;
				}
				path.push(node);
				match parents[node] {
					Some(parent) => node = parent,
					None => break node,
				}
			};
			for node in path.drain(..) {
				root_indices[node] = Some(root);
			}
			data.organization
				.root_membership
				.get(&data.entities[root].key)
				.cloned()
		})
		.collect()
}
