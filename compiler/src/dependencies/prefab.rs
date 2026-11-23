//! Discover runtime prefab documents and rewrite packaged prefab references.

use std::collections::HashMap;

use engine_core::{
	asset_catalog::{AssetKind, CATALOG_FILE},
	components::{ScriptIssue, ScriptReference},
	editor::PrefabData,
};

use super::{Closure, HashSet, Path, PathBuf, ProjectData, Read, checked_asset_path, fs};

impl Closure<'_> {
	pub(super) fn prefab(&mut self, relative: &Path) -> Result<(), String> {
		let relative = self.key(relative)?;
		if self.prefab_stack.contains(&relative) {
			return Err(
				"Cyclic runtime prefab dependency; remove the recursive spawner reference".into(),
			);
		}
		if self.prefabs.contains(&relative) {
			return Ok(());
		}
		if self.prefab_stack.len() >= 32 {
			return Err("Runtime prefab dependencies exceed 32 levels".into());
		}
		self.prefab_stack.insert(relative.clone());
		let result = self.prefab_contents(&relative);
		self.prefab_stack.remove(&relative);
		if result.is_ok() {
			self.prefabs.insert(relative);
		}
		result
	}

	pub(super) fn prefab_dependency(&mut self, path: &str) -> Result<(), String> {
		let asset = self.catalog.by_path(path).cloned();
		if let Some(asset) = asset {
			if asset.kind != AssetKind::Prefab {
				return Err("Runtime spawner reference resolves to a non-prefab asset".into());
			}
			self.record(&asset.id)?;
			self.prefab(Path::new(&asset.path))
		} else {
			self.prefab(Path::new(path))
		}
	}

	fn prefab_contents(&mut self, relative: &Path) -> Result<(), String> {
		let bytes = self.file(relative)?;
		let mut prefab =
			PrefabData::from_json(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)?;
		if let Some(gameplay) = &prefab.gameplay {
			let keys: HashSet<_> = prefab.entities.iter().map(|entity| &entity.key).collect();
			for key in gameplay.entity_references.values() {
				if !keys.contains(key) {
					return Err(format!(
						"Prefab has unresolved external entity {}",
						key.as_str()
					));
				}
			}
			for id in gameplay.scene_references.values() {
				if id != &gameplay.source_scene {
					self.project.scenes.get(id).map_err(|_| {
						format!("Prefab destination scene {} is missing", id.as_str())
					})?;
				}
			}
			if let Some(settings) = &gameplay.scene_settings {
				let mut module_project = self.project.clone();
				module_project.gameplay_module = gameplay.module.clone();
				module_project.gameplay_settings = None;
				let references =
					gameplay_catalog::resolve(&module_project)?.prefab_references(settings)?;
				if references.entities != gameplay.entity_references
					|| references.scenes != gameplay.scene_references
				{
					return Err("Prefab setting references disagree with its native module".into());
				}
			}
		}
		if !prefab.composition.is_empty() {
			prefab.entities = prefab.expanded_entities()?.into_owned();
			prefab.composition = Default::default();
		}
		validate_scripts(&prefab)
			.map_err(|error| format!("Prefab {}: {error}", relative.display()))?;
		self.entities(&prefab.entities)?;
		self.generate(relative.to_path_buf(), prefab.to_json()?.into_bytes())?;
		Ok(())
	}
}

fn validate_scripts(prefab: &PrefabData) -> Result<(), String> {
	let entities: HashMap<_, _> = prefab
		.entities
		.iter()
		.map(|entity| (&entity.key, entity))
		.collect();
	let slots: HashSet<_> = prefab.reference_slots.values().collect();
	let descriptors = gameplay_catalog::script_descriptors();
	for entity in &prefab.entities {
		for script in entity.scripts.iter().filter(|script| script.enabled) {
			let result = descriptors
				.iter()
				.find(|descriptor| descriptor.id == script.settings.script_id)
				.ok_or_else(|| ScriptIssue::MissingRegistration(script.settings.script_id.clone()))
				.and_then(|descriptor| {
					descriptor.validate_attachment(
						entity,
						&script.settings,
						|key| entities.get(key).copied(),
						|reference| match reference {
							ScriptReference::Entity { entity } => slots.contains(entity),
							ScriptReference::Slot { slot } => {
								prefab.reference_slots.contains_key(slot)
							}
						},
					)
				});
			result.map_err(|issue| issue.at_attachment(&entity.key, &script.id))?;
		}
	}
	Ok(())
}

#[cfg(test)]
pub(super) fn prefab_paths_with_budget(
	root: &Path,
	project: &ProjectData,
	byte_budget: u64,
) -> Result<Vec<PathBuf>, String> {
	prefab_paths_excluding(root, project, byte_budget, &HashSet::new())
}
pub(super) fn prefab_paths_excluding(
	root: &Path,
	project: &ProjectData,
	byte_budget: u64,
	ignored: &HashSet<String>,
) -> Result<Vec<PathBuf>, String> {
	let scene_paths = project
		.scenes
		.iter()
		.map(|entry| {
			checked_asset_path(root, Path::new(entry.asset_path()))
				.map_err(|error| error.to_string())
		})
		.collect::<Result<HashSet<_>, _>>()?;
	let mut pending = vec![(root.to_path_buf(), 0usize)];
	let mut visited = 0usize;
	let mut scanned_bytes = 0u64;
	let mut result = Vec::new();
	while let Some((path, depth)) = pending.pop() {
		visited += 1;
		if visited > 10_000 || depth > 32 {
			return Err("Prefab inventory exceeds its traversal budget".into());
		}
		let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
		if ignored.contains(
			&relative
				.to_string_lossy()
				.replace('\\', "/")
				.to_ascii_lowercase(),
		) {
			continue;
		}
		let path = checked_asset_path(root, relative).map_err(|error| error.to_string())?;
		let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
		if metadata.is_dir() {
			for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
				pending.push((entry.map_err(|error| error.to_string())?.path(), depth + 1));
				if pending.len() + visited > 10_000 {
					return Err("Prefab inventory exceeds its traversal budget".into());
				}
			}
			continue;
		}
		if !metadata.is_file()
			|| !path
				.extension()
				.and_then(|value| value.to_str())
				.is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
			|| scene_paths.contains(&path)
		{
			continue;
		}
		let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
		if relative.components().count() == 1
			&& path
				.file_name()
				.and_then(|value| value.to_str())
				.is_some_and(|name| name.eq_ignore_ascii_case(CATALOG_FILE))
		{
			continue;
		}
		if path
			.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name.to_ascii_lowercase().ends_with(".clip.json"))
		{
			continue;
		}
		let strict_prefab_folder = relative
			.components()
			.next()
			.and_then(|component| component.as_os_str().to_str())
			.is_some_and(|name| name.eq_ignore_ascii_case("prefabs"));
		if metadata.len() > PrefabData::MAX_JSON_BYTES as u64 {
			if strict_prefab_folder {
				return Err(format!(
					"Prefab {} exceeds its JSON size limit",
					relative.display()
				));
			}
			continue;
		}
		scanned_bytes = scanned_bytes
			.checked_add(metadata.len())
			.filter(|bytes| *bytes <= byte_budget)
			.ok_or("Prefab inventory exceeds its JSON byte budget")?;
		let mut bytes = Vec::new();
		fs::File::open(&path)
			.map_err(|error| error.to_string())?
			.take(metadata.len() + 1)
			.read_to_end(&mut bytes)
			.map_err(|error| error.to_string())?;
		if bytes.len() as u64 != metadata.len() {
			return Err("JSON changed during prefab discovery".into());
		}
		let recognizable = serde_json::from_slice::<serde_json::Value>(&bytes)
			.ok()
			.is_some_and(|value| {
				value.is_object()
					&& value.get("schema_version").is_some()
					&& value.get("entities").is_some()
					&& value.get("camera").is_none()
					&& value.get("bindings").is_none()
			});
		if strict_prefab_folder || recognizable {
			result.push(relative.to_owned());
		}
	}
	result.sort();
	Ok(result)
}
