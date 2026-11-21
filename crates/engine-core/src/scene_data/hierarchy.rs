use super::{EntityKey, MAX_SCENE_ENTITIES};
use glam::Mat4;
use std::collections::HashMap;

/// Linear-time indexed parent traversal shared by CPU scene data and GPU scenes.
pub fn resolve_world_matrices(
	local: &[Mat4],
	keys: &[Option<&EntityKey>],
	parents: &[Option<&EntityKey>],
) -> Result<Vec<Mat4>, String> {
	if local.len() != keys.len() || local.len() != parents.len() || local.len() > MAX_SCENE_ENTITIES
	{
		return Err(
			"Hierarchy arrays must have matching lengths within the scene entity limit".into(),
		);
	}
	let mut indices = HashMap::with_capacity(keys.len());
	for (index, key) in keys.iter().enumerate() {
		if let Some(key) = key
			&& indices.insert(key.as_str(), index).is_some()
		{
			return Err("Duplicate entity key in hierarchy".into());
		}
	}
	let mut parent_indices = Vec::with_capacity(local.len());
	for parent in parents {
		parent_indices.push(match parent {
			Some(key) => Some(
				*indices
					.get(key.as_str())
					.ok_or_else(|| format!("Parent key {:?} does not exist", key.as_str()))?,
			),
			None => None,
		});
	}
	let mut states = vec![0u8; local.len()];
	let mut world = vec![Mat4::IDENTITY; local.len()];
	let mut path = Vec::new();
	for index in 0..local.len() {
		if states[index] == 2 {
			continue;
		}
		let mut current = Some(index);
		while let Some(node) = current {
			match states[node] {
				2 => break,
				1 => return Err("Scene hierarchy contains a parent cycle".into()),
				_ => {
					states[node] = 1;
					path.push(node);
					current = parent_indices[node];
				}
			}
		}
		while let Some(node) = path.pop() {
			world[node] =
				parent_indices[node].map_or(local[node], |parent| world[parent] * local[node]);
			if !world[node].is_finite() {
				return Err("Hierarchy produces a nonfinite world transform".into());
			}
			states[node] = 2;
		}
	}
	Ok(world)
}
