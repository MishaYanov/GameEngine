//! Export only the closure of catalog scenes and saved prefab assemblies.
use super::preflight::{append_scene_error, ordered_scenes, scene_error};
use super::{
	Arc, Digest, PackageAsset, Path, PathBuf, ProjectData, Read, Sha256, checked_asset_path, fs, io,
};
use engine_core::{
	asset_catalog::{AssetCatalog, AssetId, AssetKind},
	scene_data::{EntityData, SceneData},
};
use std::collections::{BTreeMap, HashMap, HashSet};

mod catalog;
mod model;
mod prefab;

pub(super) use catalog::read_catalog;
use prefab::prefab_paths_excluding;

const MAX_URI_EDGES: usize = 100_000;

pub(super) struct DependencySnapshot {
	pub files: Vec<PackageAsset>,
	pub source_files: Vec<PackageAsset>,
	pub generated_files: BTreeMap<PathBuf, Vec<u8>>,
	pub catalog_source: Option<PackageAsset>,
	pub catalog_output: Option<Vec<u8>>,
}

struct Closure<'a> {
	root: &'a Path,
	project: &'a ProjectData,
	catalog: AssetCatalog,
	files: BTreeMap<PathBuf, PackageAsset>,
	ids: HashSet<AssetId>,
	models: HashMap<PathBuf, usize>,
	animations: HashMap<AssetId, Arc<engine_core::animation::AnimationAsset>>,
	animation_bytes: usize,
	prefabs: HashSet<PathBuf>,
	prefab_stack: HashSet<PathBuf>,
	generated_files: BTreeMap<PathBuf, Vec<u8>>,
	generated_bytes: u64,
	linked_sources: HashSet<String>,
	resolved_paths: HashMap<PathBuf, PathBuf>,
	bytes: u64,
	depth: usize,
	uri_edges: usize,
	#[cfg(test)]
	file_reads: usize,
}
impl Closure<'_> {
	fn generate(&mut self, path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
		let prior = self
			.generated_files
			.get(&path)
			.map_or(0, |bytes| bytes.len() as u64);
		let total = self.generated_bytes - prior + bytes.len() as u64;
		if bytes.len() > 16 * 1024 * 1024 || total > 512 * 1024 * 1024 {
			return Err(
				"Resolved JSON output exceeds its 16 MiB file or 512 MiB aggregate budget".into(),
			);
		}
		self.generated_bytes = total;
		self.generated_files.insert(path, bytes);
		Ok(())
	}
	fn key(&mut self, relative: &Path) -> Result<PathBuf, String> {
		if let Some(key) = self.resolved_paths.get(relative) {
			return Ok(key.clone());
		}
		if relative.as_os_str().len() > 1024 {
			return Err("Dependency path exceeds 1024 characters".into());
		}
		if self.resolved_paths.len() >= MAX_URI_EDGES {
			return Err("Dependency closure exceeds 100000 distinct path spellings".into());
		}
		let path = checked_asset_path(self.root, relative)
			.map_err(|error| format!("Dependency {}: {error}", relative.display()))?;
		let key = path
			.strip_prefix(self.root)
			.map_err(|error| error.to_string())?
			.to_path_buf();
		self.resolved_paths.insert(relative.to_owned(), key.clone());
		Ok(key)
	}
	fn file(&mut self, relative: &Path) -> Result<Vec<u8>, String> {
		let relative = self.key(relative)?;
		let path = checked_asset_path(self.root, &relative)
			.map_err(|error| format!("Dependency {}: {error}", relative.display()))?;
		let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
		let extension = relative
			.extension()
			.and_then(|value| value.to_str())
			.unwrap_or("")
			.to_ascii_lowercase();
		if !metadata.is_file()
			|| metadata.len() > 64 * 1024 * 1024
			|| (extension == "json" && metadata.len() > 16 * 1024 * 1024)
		{
			return Err(format!(
				"Dependency {} exceeds its regular-file size limit",
				relative.display()
			));
		}
		if !["gltf", "glb", "bin", "png", "jpg", "jpeg", "json"].contains(&extension.as_str()) {
			return Err(format!(
				"Unsupported dependency format: {}",
				relative.display()
			));
		}
		self.check_budget(&relative, metadata.len())?;
		let mut bytes = Vec::new();
		#[cfg(test)]
		{
			self.file_reads += 1;
		}
		fs::File::open(&path)
			.map_err(|error| error.to_string())?
			.take(metadata.len() + 1)
			.read_to_end(&mut bytes)
			.map_err(|error| error.to_string())?;
		if bytes.len() as u64 != metadata.len() {
			return Err("Dependency changed during preflight".into());
		}
		self.record_bytes(relative, &bytes)?;
		Ok(bytes)
	}
	fn check_budget(&self, relative: &Path, len: u64) -> Result<(), String> {
		if !self.files.contains_key(relative)
			&& (self.files.len() >= 10_000 || self.bytes.saturating_add(len) > 512 * 1024 * 1024)
		{
			return Err("Dependency closure exceeds 10000 files or 512 MiB".into());
		}
		Ok(())
	}
	fn record_bytes(&mut self, relative: PathBuf, bytes: &[u8]) -> Result<(), String> {
		let len = bytes.len() as u64;
		self.check_budget(&relative, len)?;
		let asset = PackageAsset {
			relative: relative.clone(),
			bytes: len,
			fingerprint: Sha256::digest(bytes).into(),
		};
		if let Some(previous) = self.files.get(&relative) {
			if previous != &asset {
				return Err("Dependency changed during preflight".into());
			}
		} else {
			self.bytes += len;
			self.files.insert(relative, asset);
		}
		Ok(())
	}
	fn entities(&mut self, entities: &[EntityData]) -> Result<(), String> {
		for entity in entities {
			if let Some(component) = &entity.animation {
				let id = &component.settings.source.asset_id;
				let source = self
					.catalog
					.by_id(id)
					.ok_or("Animation source identity is missing from the catalog")?
					.clone();
				if source.kind != AssetKind::Model {
					return Err("Animation source must be a model asset".into());
				}
				let asset = if let Some(asset) = self.animations.get(id) {
					Arc::clone(asset)
				} else {
					let asset =
						bridge::animation_asset::load(self.root, &component.settings.source)?;
					self.animation_bytes = self
						.animation_bytes
						.checked_add(asset.retained_bytes())
						.filter(|bytes| *bytes <= 128 * 1024 * 1024)
						.ok_or("Export animation preparation exceeds 128 MiB")?;
					self.animations.insert(id.clone(), Arc::clone(&asset));
					asset
				};
				asset.validate_settings(&component.settings)?;
				let mesh = self.catalog.by_path(&entity.model_path);
				if !entity.has_mesh_component()
					|| mesh.is_none_or(|mesh| &mesh.id != id)
					|| entity.asset_id.as_ref().is_some_and(|mesh| mesh != id)
					|| entity.mesh_index.unwrap_or(0) != asset.skin.mesh_index as usize
				{
					return Err(
						"Animation requires its source model and selected mesh on the same entity"
							.into(),
					);
				}
				self.record(id)?;
			}
			if let Some(link) = &entity.linked_prefab {
				let path = self
					.catalog
					.by_id(&link.source.source_id)
					.map_or(link.source.path.as_str(), |asset| asset.path.as_str());
				self.linked_sources.insert(path.to_ascii_lowercase());
				self.linked_sources
					.insert(link.source.path.to_ascii_lowercase());
			}
			for component in entity
				.native_behaviors
				.iter()
				.filter(|component| component.enabled)
			{
				if let engine_core::components::NativeBehaviorSpec::TimedSpawner {
					prefab_path,
					..
				} = &component.behavior
				{
					self.prefab_dependency(prefab_path)?;
				}
			}
			if !entity.has_mesh_component() {
				continue;
			}
			let resolved = self
				.catalog
				.resolve(
					entity.asset_id.as_ref(),
					&entity.model_path,
					entity.mesh_index,
				)?
				.cloned();
			if let Some(asset) = &resolved {
				if asset.kind != AssetKind::Model {
					return Err("Entity references a non-model asset identity".into());
				}
				self.record(&asset.id)?;
			}
			let path = resolved
				.as_ref()
				.map_or(entity.model_path.as_str(), |asset| asset.path.as_str());
			// Invisible logical markers may intentionally have no loadable model.
			if !entity.renderable && resolved.is_none() && !self.root.join(path).is_file() {
				continue;
			}
			let relative = Path::new(path);
			if !relative
				.extension()
				.and_then(|value| value.to_str())
				.is_some_and(|extension| {
					extension.eq_ignore_ascii_case("glb") || extension.eq_ignore_ascii_case("gltf")
				}) {
				if !entity.renderable {
					continue;
				}
				return Err(format!(
					"Renderable model '{}' must use glTF (.gltf or .glb)",
					entity.model_path
				));
			}
			self.model(relative, entity.mesh_index).map_err(|error| {
				format!("Model '{}' preflight failed: {error}", entity.model_path)
			})?;
		}
		Ok(())
	}
}

// Old authoring allowed saving a prefab at any relative JSON path. Discover
// recognizable assemblies globally without adding unrelated JSON to the export.
pub(super) fn prepare(project: &ProjectData, root: &Path) -> Result<DependencySnapshot, String> {
	let (catalog, catalog_source) = catalog::capture_catalog(root)?;
	let mut closure = Closure {
		root,
		project,
		catalog,
		files: BTreeMap::new(),
		ids: HashSet::new(),
		models: HashMap::new(),
		animations: HashMap::new(),
		animation_bytes: 0,
		prefabs: HashSet::new(),
		prefab_stack: HashSet::new(),
		generated_files: BTreeMap::new(),
		generated_bytes: 0,
		linked_sources: HashSet::new(),
		resolved_paths: HashMap::new(),
		bytes: 0,
		depth: 0,
		uri_edges: 0,
		#[cfg(test)]
		file_reads: 0,
	};
	if let Some(audio) = bridge::feedback_audio::load(root)? {
		audio.mix().validate_scenes(&project.scenes)?;
		for (path, bytes) in audio.files() {
			let relative = closure.key(Path::new(path))?;
			closure.record_bytes(relative, bytes)?;
		}
	}
	let mut report = String::new();
	let mut truncated = false;
	for entry in ordered_scenes(project) {
		let result = (|| {
			let bytes = closure.file(Path::new(entry.asset_path()))?;
			let scene = SceneData::from_json(
				std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
			)?;
			let keys: HashSet<_> = scene.entities.iter().map(|entity| &entity.key).collect();
			for (index, behavior) in project.behaviors_for(entry.id()).enumerate() {
				if !keys.contains(behavior.entity()) {
					return Err(format!(
						"Behavior {index} targets missing entity {:?}",
						behavior.entity().as_str()
					));
				}
			}
			closure.entities(&scene.entities)?;
			if scene
				.entities
				.iter()
				.any(|entity| entity.linked_prefab.is_some())
			{
				let key = closure.key(Path::new(entry.asset_path()))?;
				closure.generate(
					key,
					engine_core::linked_prefab::resolve_scene(&scene)
						.to_json()?
						.into_bytes(),
				)?;
			}
			Ok::<_, String>(())
		})();
		if let Err(error) = result {
			append_scene_error(&mut report, &scene_error(entry, error), &mut truncated);
		}
	}
	if !report.is_empty() {
		return Err(report);
	}
	for path in prefab_paths_excluding(root, project, 512 * 1024 * 1024, &closure.linked_sources)? {
		if closure.linked_sources.contains(
			&path
				.to_string_lossy()
				.replace('\\', "/")
				.to_ascii_lowercase(),
		) {
			continue;
		}
		closure
			.prefab(&path)
			.map_err(|error| format!("Prefab {}: {error}", path.display()))?;
	}
	// Include metadata for any dependency addressed by path, not only explicit IDs.
	let catalog_paths: Vec<_> = closure
		.catalog
		.assets
		.iter()
		.filter_map(|asset| {
			let path = checked_asset_path(root, Path::new(&asset.path)).ok()?;
			Some((
				asset.id.clone(),
				path.strip_prefix(root).ok()?.to_path_buf(),
			))
		})
		.collect();
	loop {
		let ids = catalog_paths
			.iter()
			.filter(|(id, path)| closure.files.contains_key(path) && !closure.ids.contains(id))
			.map(|(id, _)| id.clone())
			.collect::<Vec<_>>();
		if ids.is_empty() {
			break;
		}
		for id in ids {
			closure.record(&id)?;
		}
	}
	let source_files: Vec<_> = closure.files.values().cloned().collect();
	for (relative, bytes) in &closure.generated_files {
		if bytes.len() > 16 * 1024 * 1024 {
			return Err("Generated JSON exceeds the 16 MiB package limit".into());
		}
		closure.files.insert(
			relative.clone(),
			PackageAsset {
				relative: relative.clone(),
				bytes: bytes.len() as u64,
				fingerprint: Sha256::digest(bytes).into(),
			},
		);
	}
	let catalog_output = if catalog_source.is_some() {
		Some(closure.export_catalog()?)
	} else {
		None
	};
	if closure.files.values().map(|file| file.bytes).sum::<u64>() > 512 * 1024 * 1024 {
		return Err("Resolved dependency package exceeds 512 MiB".into());
	}
	Ok(DependencySnapshot {
		files: closure.files.into_values().collect(),
		source_files,
		generated_files: closure.generated_files,
		catalog_source,
		catalog_output,
	})
}

#[cfg(test)]
#[path = "../tests/unit/dependencies.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/dependencies/discovery_tests.rs"]
mod discovery_tests;
