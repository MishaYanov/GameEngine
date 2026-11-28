use std::{
	collections::{HashMap, HashSet, VecDeque},
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant},
};

use super::AssetManagerError;
use crate::assets::{
	ImportedScene,
	gltf_loader::{self, Resources, load_resources_in_root},
	project_assets::open_warm_file,
	warm_model::matches_current_bytes,
};

const SCENE_CACHE_ENTRIES: usize = 8;
const SCENE_CACHE_LIMIT: usize = 32 * 1024 * 1024;
const RESOURCE_CACHE_ENTRIES: usize = 4;
const RECENT_IDLE_MIN_BYTES: usize = 8 * 1024 * 1024;
const RECENT_IDLE_MAX_BYTES: usize = 128 * 1024 * 1024;
const RECENT_MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const RECENT_IDLE_TTL: Duration = Duration::from_secs(30);

/// Keep CPU scene metadata and source snapshots under their respective FIFO budgets.
#[derive(Default)]
pub(super) struct SourceCache {
	scenes: HashMap<PathBuf, Arc<ImportedScene>>,
	scene_order: VecDeque<PathBuf>,
	scene_bytes: usize,
	resource_cache: HashMap<PathBuf, (Resources, usize)>,
	resource_order: VecDeque<PathBuf>,
	resource_bytes: usize,
	recent: Option<RecentSource>,
}

struct RecentSource {
	path: PathBuf,
	file_bytes: u64,
	since: Instant,
}

impl SourceCache {
	pub(super) fn retained_bytes(&self) -> usize {
		self.scene_bytes + self.resource_bytes
	}

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
		self.resolve_idle_source(path);
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

	fn resolve_idle_source(&mut self, requested: &Path) {
		let Some(recent) = self.recent.take() else {
			return;
		};
		if recent.since.elapsed() < RECENT_IDLE_TTL {
			if recent.path == requested {
				// Re-enter active ownership only after checking the exact source
				// loaded on the last visit. Embedded URIs are covered by this hash.
				if self
					.resource_cache
					.get(requested)
					.is_some_and(|(resources, _)| {
						matches_current_bytes(
							requested,
							recent.file_bytes,
							&resources.source_fingerprint,
						)
					}) {
					return;
				}
			} else if self.resource_cache.contains_key(requested) {
				self.recent = Some(recent);
				return;
			}
		}
		// Drop idle CPU bytes before decoding a different cold source.
		self.remove(&recent.path);
	}

	pub(super) fn remap(&mut self, from: &Path, to: &Path) {
		if self
			.recent
			.as_ref()
			.is_some_and(|recent| recent.path == from || recent.path == to)
			&& let Some(recent) = self.recent.take()
		{
			self.remove(&recent.path);
		}
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
		if self
			.recent
			.as_ref()
			.is_some_and(|recent| recent.path == path)
		{
			self.recent = None;
		}
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
		self.recent = None;
	}

	pub(super) fn prune_unused(&mut self, retained_paths: &HashSet<PathBuf>, retain_recent: bool) {
		self.scenes
			.retain(|path, scene| Arc::strong_count(scene) > 1 || retained_paths.contains(path));
		self.scene_order
			.retain(|path| self.scenes.contains_key(path));
		self.recount_scene_bytes();
		let mut previous = self.recent.take();
		let recent = retain_recent
			.then(|| {
				self.resource_order.iter().rev().find_map(|path| {
					if retained_paths.contains(path) || self.scenes.contains_key(path) {
						return None;
					}
					if previous.as_ref().is_some_and(|entry| &entry.path == path) {
						return previous
							.take()
							.filter(|entry| entry.since.elapsed() < RECENT_IDLE_TTL);
					}
					let (resources, bytes) = self.resource_cache.get(path)?;
					if !(RECENT_IDLE_MIN_BYTES..=RECENT_IDLE_MAX_BYTES).contains(bytes)
						|| !path
						.extension()
						.is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
						|| !resources.is_embedded_static()
					{
						return None;
					}
					let file_bytes = open_warm_file(path).ok()?.metadata().ok()?.len();
					(file_bytes <= RECENT_MAX_FILE_BYTES).then(|| RecentSource {
						path: path.clone(),
						file_bytes,
						since: Instant::now(),
					})
				})
			})
			.flatten();
		self.resource_cache.retain(|path, _| {
			retained_paths.contains(path)
				|| self.scenes.contains_key(path)
				|| recent.as_ref().is_some_and(|entry| &entry.path == path)
		});
		self.recent = recent;
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
