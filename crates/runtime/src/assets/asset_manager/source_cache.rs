use std::{
	collections::{HashMap, HashSet, VecDeque},
	path::{Path, PathBuf},
	sync::Arc,
};

use super::AssetManagerError;
use crate::assets::{
	ImportedScene,
	gltf_loader::{self, Resources, load_resources_in_root},
};

const SCENE_CACHE_ENTRIES: usize = 8;
const SCENE_CACHE_LIMIT: usize = 32 * 1024 * 1024;
const RESOURCE_CACHE_ENTRIES: usize = 4;

/// Keep CPU scene metadata and source snapshots under their respective FIFO budgets.
#[derive(Default)]
pub(super) struct SourceCache {
	scenes: HashMap<PathBuf, Arc<ImportedScene>>,
	scene_order: VecDeque<PathBuf>,
	scene_bytes: usize,
	resource_cache: HashMap<PathBuf, (Resources, usize)>,
	resource_order: VecDeque<PathBuf>,
	resource_bytes: usize,
}

impl SourceCache {
	pub(super) fn load_scene(
		&mut self,
		root: &Path,
		full_path: PathBuf,
	) -> Result<Arc<ImportedScene>, AssetManagerError> {
		if let Some(scene) = self.scenes.get(&full_path) {
			return Ok(Arc::clone(scene));
		}
		let scene = Arc::new(gltf_loader::load_scene_in_root(&full_path, root)?);
		let bytes = scene_size(&full_path, &scene);
		if bytes > SCENE_CACHE_LIMIT {
			return Ok(scene);
		}
		while self.scenes.len() >= SCENE_CACHE_ENTRIES
			|| self.scene_bytes + bytes > SCENE_CACHE_LIMIT
		{
			if let Some(oldest) = self.scene_order.pop_front()
				&& let Some(scene) = self.scenes.remove(&oldest)
			{
				self.scene_bytes -= scene_size(&oldest, &scene);
			}
		}
		self.scenes.insert(full_path.clone(), Arc::clone(&scene));
		self.scene_order.push_back(full_path);
		self.scene_bytes += bytes;
		Ok(scene)
	}

	pub(super) fn prepare_resources(
		&mut self,
		root: &Path,
		path: &Path,
		mesh_index: usize,
	) -> Result<&Resources, AssetManagerError> {
		if let Some((resources, bytes)) = self.resource_cache.get_mut(path) {
			resources.prepare_mesh_images(mesh_index)?;
			let next_bytes = resources.retained_bytes();
			self.resource_bytes = self.resource_bytes - *bytes + next_bytes;
			*bytes = next_bytes;
			// Preparing another mesh can grow an existing entry. Keep that
			// entry while applying the same aggregate limit to other files.
			self.resource_order.retain(|cached| cached != path);
			self.resource_order.push_back(path.to_path_buf());
		} else {
			let mut resources = load_resources_in_root(path, root)?;
			resources.prepare_mesh_images(mesh_index)?;
			let bytes = resources.retained_bytes();
			self.resource_order.push_back(path.to_path_buf());
			self.resource_bytes += bytes;
			self.resource_cache
				.insert(path.to_path_buf(), (resources, bytes));
		}
		while self.resource_cache.len() > RESOURCE_CACHE_ENTRIES
			|| self.resource_bytes > gltf_loader::IMPORT_LIMIT
		{
			if let Some(oldest) = self.resource_order.pop_front()
				&& let Some((_, bytes)) = self.resource_cache.remove(&oldest)
			{
				self.resource_bytes -= bytes;
			}
		}
		Ok(&self.resource_cache[path].0)
	}

	pub(super) fn remap(&mut self, from: &Path, to: &Path) {
		if let Some(value) = self.resource_cache.remove(from) {
			self.resource_cache.insert(to.to_path_buf(), value);
		}
		for path in &mut self.resource_order {
			if path == from {
				*path = to.to_path_buf();
			}
		}
		if let Some(value) = self.scenes.remove(from) {
			self.scenes.insert(to.to_path_buf(), value);
		}
		for path in &mut self.scene_order {
			if path == from {
				*path = to.to_path_buf();
			}
		}
		self.recount_scene_bytes();
	}

	pub(super) fn remove(&mut self, path: &Path) {
		self.scenes.remove(path);
		self.scene_order.retain(|key| key != path);
		self.recount_scene_bytes();
		if let Some((_, bytes)) = self.resource_cache.remove(path) {
			self.resource_bytes -= bytes;
		}
		self.resource_order.retain(|key| key != path);
	}

	pub(super) fn clear(&mut self) {
		self.scenes.clear();
		self.scene_order.clear();
		self.scene_bytes = 0;
		self.resource_cache.clear();
		self.resource_order.clear();
		self.resource_bytes = 0;
	}

	pub(super) fn prune_unused(&mut self, retained_paths: &HashSet<PathBuf>) {
		self.scenes
			.retain(|path, scene| Arc::strong_count(scene) > 1 || retained_paths.contains(path));
		self.scene_order
			.retain(|path| self.scenes.contains_key(path));
		self.recount_scene_bytes();
		self.resource_cache
			.retain(|path, _| retained_paths.contains(path) || self.scenes.contains_key(path));
		self.resource_order
			.retain(|path| self.resource_cache.contains_key(path));
		self.resource_bytes = self.resource_cache.values().map(|(_, bytes)| bytes).sum();
	}

	fn recount_scene_bytes(&mut self) {
		self.scene_bytes = self
			.scenes
			.iter()
			.map(|(path, scene)| scene_size(path, scene))
			.sum();
	}
}

fn scene_size(path: &Path, scene: &ImportedScene) -> usize {
	scene.retained_bytes() + path.as_os_str().len() * 4 + 128
}

#[cfg(test)]
#[path = "../../../tests/unit/assets/asset_manager/source_cache.rs"]
mod tests;
