//! Export only the closure of catalog scenes and saved prefab assemblies.
use super::*;
use engine_core::{
	asset_catalog::{AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES},
	editor::PrefabData,
	scene_data::EntityData,
};
use std::collections::BTreeMap;

const MAX_URI_EDGES: usize = 100_000;

pub(super) struct DependencySnapshot {
	pub files: Vec<PackageAsset>,
	pub catalog_source: Option<PackageAsset>,
	pub catalog_output: Option<Vec<u8>>,
}

pub(super) fn read_catalog(root: &Path) -> Result<AssetCatalog, String> {
	let relative = Path::new(CATALOG_FILE);
	match fs::symlink_metadata(root.join(relative)) {
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AssetCatalog::default()),
		Err(error) => Err(error.to_string()),
		Ok(_) => {
			let path = checked_asset_path(root, relative).map_err(|error| error.to_string())?;
			let mut bytes = String::new();
			fs::File::open(path)
				.map_err(|error| error.to_string())?
				.take(MAX_CATALOG_BYTES as u64 + 1)
				.read_to_string(&mut bytes)
				.map_err(|error| error.to_string())?;
			AssetCatalog::from_json(&bytes)
		}
	}
}

struct Closure<'a> {
	root: &'a Path,
	project: &'a ProjectData,
	catalog: AssetCatalog,
	files: BTreeMap<PathBuf, PackageAsset>,
	ids: HashSet<AssetId>,
	models: HashMap<PathBuf, usize>,
	resolved_paths: HashMap<PathBuf, PathBuf>,
	bytes: u64,
	depth: usize,
	uri_edges: usize,
	#[cfg(test)]
	file_reads: usize,
}
impl Closure<'_> {
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
		if relative.as_os_str().len() > 1024 {
			return Err("Dependency path exceeds 1024 characters".into());
		}
		if !self.files.contains_key(&relative) {
			if self.files.len() == 10_000
				|| self.bytes.saturating_add(metadata.len()) > 512 * 1024 * 1024
			{
				return Err("Dependency closure exceeds 10000 files or 512 MiB".into());
			}
			self.bytes += metadata.len();
		}
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
		let asset = PackageAsset {
			relative: relative.to_owned(),
			bytes: metadata.len(),
			fingerprint: Sha256::digest(&bytes).into(),
		};
		if let Some(previous) = self.files.get(&relative) {
			if previous != &asset {
				return Err("Dependency changed during preflight".into());
			}
		} else {
			self.files.insert(relative.to_owned(), asset);
		}
		Ok(bytes)
	}
	fn capture_uri(&mut self, model: &Path, uri: &str) -> Result<(), String> {
		if self.uri_edges == MAX_URI_EDGES {
			return Err("Model dependency closure exceeds 100000 URI edges".into());
		}
		self.uri_edges += 1;
		if let Some(path) = uri_path(model, uri)? {
			if path == Path::new(CATALOG_FILE)
				|| (cfg!(windows) && path.to_string_lossy().eq_ignore_ascii_case(CATALOG_FILE))
			{
				return Err(
					"Model dependencies cannot reference reserved root asset-catalog.json metadata"
						.into(),
				);
			}
			let path = self.key(&path)?;
			if path == Path::new(CATALOG_FILE)
				|| (cfg!(windows) && path.to_string_lossy().eq_ignore_ascii_case(CATALOG_FILE))
			{
				return Err(
					"Model dependencies cannot reference reserved root asset-catalog.json metadata"
						.into(),
				);
			}
			// Capture source bytes once. Every later stage still verifies the
			// fingerprint before and during its exclusive output copy.
			if !self.files.contains_key(&path) {
				self.file(&path)?;
			}
		}
		Ok(())
	}
	fn record(&mut self, id: &AssetId) -> Result<(), String> {
		if !self.ids.insert(id.clone()) {
			return Ok(());
		}
		if self.depth == 64 {
			return Err("Asset dependency nesting exceeds 64 levels".into());
		}
		self.depth += 1;
		let result = self.record_contents(id);
		self.depth -= 1;
		result
	}
	fn record_contents(&mut self, id: &AssetId) -> Result<(), String> {
		let asset = self
			.catalog
			.by_id(id)
			.ok_or("Unresolved dependency asset identity")?
			.clone();
		let bytes = self.file(Path::new(&asset.path))?;
		if bytes.len() as u64 != asset.bytes
			|| format!("{:x}", Sha256::digest(&bytes)) != asset.fingerprint
		{
			return Err(format!(
				"Asset {} changed outside its catalog; reimport it before export",
				asset.path
			));
		}
		for dependency in &asset.dependencies {
			self.record(dependency)?;
		}
		match asset.kind {
			AssetKind::Model => {
				self.model(Path::new(&asset.path), None)?;
			}
			AssetKind::Prefab => self.prefab(Path::new(&asset.path))?,
			AssetKind::Scene => {
				let scene = SceneData::from_json(
					std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
				)?;
				self.entities(&scene.entities)?;
			}
			AssetKind::Texture => {}
		}
		Ok(())
	}
	fn entities(&mut self, entities: &[EntityData]) -> Result<(), String> {
		for entity in entities {
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
	fn model(&mut self, relative: &Path, mesh: Option<usize>) -> Result<(), String> {
		let relative = self.key(relative)?;
		if let Some(count) = self.models.get(&relative) {
			return if mesh.unwrap_or(0) < *count {
				Ok(())
			} else {
				Err("Referenced mesh index is unavailable".into())
			};
		}
		let bytes = self.file(&relative)?;
		let model =
			gltf::Gltf::from_slice(&bytes).map_err(|error| format!("Invalid glTF: {error}"))?;
		let count = model.meshes().count();
		if count > 10_000 || model.buffers().count() > 10_000 || model.images().count() > 10_000 {
			return Err("Model dependency metadata exceeds 10000 entries".into());
		}
		if mesh.unwrap_or(0) >= count {
			return Err("Referenced mesh index is unavailable".into());
		}
		self.models.insert(relative.to_owned(), count);
		let mut uris = Vec::new();
		for buffer in model.buffers() {
			if let gltf::buffer::Source::Uri(uri) = buffer.source() {
				uris.push(uri);
			}
		}
		for image in model.images() {
			if let gltf::image::Source::Uri { uri, .. } = image.source() {
				uris.push(uri);
			}
		}
		for uri in uris {
			self.capture_uri(&relative, uri)?;
		}
		Ok(())
	}
	fn prefab(&mut self, relative: &Path) -> Result<(), String> {
		let bytes = self.file(relative)?;
		let prefab =
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
					courtyard_gameplay::resolve(&module_project)?.prefab_references(settings)?;
				if references.entities != gameplay.entity_references
					|| references.scenes != gameplay.scene_references
				{
					return Err("Prefab setting references disagree with its native module".into());
				}
			}
		}
		self.entities(&prefab.entities)
	}
}

fn uri_path(model: &Path, uri: &str) -> Result<Option<PathBuf>, String> {
	if uri.starts_with("data:") {
		return Ok(None);
	}
	let decoded = urlencoding::decode(uri).map_err(|error| error.to_string())?;
	if decoded.contains([':', '\\', '\0']) || Path::new(decoded.as_ref()).is_absolute() {
		return Err("Model dependency must use a relative contained URI".into());
	}
	let mut result = model.parent().unwrap_or(Path::new("")).to_path_buf();
	for component in Path::new(decoded.as_ref()).components() {
		match component {
			std::path::Component::Normal(value) => result.push(value),
			std::path::Component::CurDir => {}
			std::path::Component::ParentDir => {
				if !result.pop() {
					return Err("Model dependency escapes the asset root".into());
				}
			}
			_ => return Err("Model dependency must be relative".into()),
		}
	}
	Ok(Some(result))
}

fn prefab_paths(root: &Path, project: &ProjectData) -> Result<Vec<PathBuf>, String> {
	prefab_paths_with_budget(root, project, 512 * 1024 * 1024)
}

// Old authoring allowed saving a prefab at any relative JSON path. Discover
// recognizable assemblies globally without adding unrelated JSON to the export.
fn prefab_paths_with_budget(
	root: &Path,
	project: &ProjectData,
	byte_budget: u64,
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
			});
		if strict_prefab_folder || recognizable {
			result.push(relative.to_owned());
		}
	}
	result.sort();
	Ok(result)
}

pub(super) fn prepare(project: &ProjectData, root: &Path) -> Result<DependencySnapshot, String> {
	let (catalog, catalog_source) = if root.join(CATALOG_FILE).exists() {
		let path =
			checked_asset_path(root, Path::new(CATALOG_FILE)).map_err(|error| error.to_string())?;
		let mut bytes = Vec::new();
		fs::File::open(path)
			.map_err(|error| error.to_string())?
			.take(MAX_CATALOG_BYTES as u64 + 1)
			.read_to_end(&mut bytes)
			.map_err(|error| error.to_string())?;
		let catalog = AssetCatalog::from_json(
			std::str::from_utf8(&bytes).map_err(|error| error.to_string())?,
		)?;
		let snapshot = PackageAsset {
			relative: CATALOG_FILE.into(),
			bytes: bytes.len() as u64,
			fingerprint: Sha256::digest(&bytes).into(),
		};
		(catalog, Some(snapshot))
	} else {
		(AssetCatalog::default(), None)
	};
	let mut closure = Closure {
		root,
		project,
		catalog,
		files: BTreeMap::new(),
		ids: HashSet::new(),
		models: HashMap::new(),
		resolved_paths: HashMap::new(),
		bytes: 0,
		depth: 0,
		uri_edges: 0,
		#[cfg(test)]
		file_reads: 0,
	};
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
			closure.entities(&scene.entities)
		})();
		if let Err(error) = result {
			append_scene_error(&mut report, &scene_error(entry, error), &mut truncated);
		}
	}
	if !report.is_empty() {
		return Err(report);
	}
	for path in prefab_paths(root, project)? {
		closure
			.prefab(&path)
			.map_err(|error| format!("Prefab {}: {error}", path.display()))?;
	}
	let prefab_ids = closure
		.catalog
		.assets
		.iter()
		.filter(|asset| asset.kind == AssetKind::Prefab)
		.map(|asset| asset.id.clone())
		.collect::<Vec<_>>();
	for id in prefab_ids {
		closure.record(&id)?;
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
	let catalog_output = if catalog_source.is_some() {
		let mut exported = closure.catalog.clone();
		exported
			.assets
			.retain(|asset| closure.ids.contains(&asset.id));
		for asset in &mut exported.assets {
			asset.source_path = None;
		}
		let bytes = exported.to_json()?.into_bytes();
		if closure.files.len() == 10_000 || closure.bytes + bytes.len() as u64 > 512 * 1024 * 1024 {
			return Err("Export catalog exceeds the package file/byte budget".into());
		}
		closure.files.insert(
			CATALOG_FILE.into(),
			PackageAsset {
				relative: CATALOG_FILE.into(),
				bytes: bytes.len() as u64,
				fingerprint: Sha256::digest(&bytes).into(),
			},
		);
		Some(bytes)
	} else {
		None
	};
	Ok(DependencySnapshot {
		files: closure.files.into_values().collect(),
		catalog_source,
		catalog_output,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn duplicate_uri_edges_read_once_and_later_source_changes_still_reject_staging() {
		let root =
			std::env::temp_dir().join(format!("export-uri-{}", AssetId::generate().as_str()));
		fs::create_dir(&root).unwrap();
		let root = root.canonicalize().unwrap();
		let project = ProjectData::load(
			Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/project.json"),
		)
		.unwrap();
		let model = serde_json::json!({
			"asset": { "version": "2.0" },
			"buffers": (0..128).map(|index| serde_json::json!({ "uri": if cfg!(windows) && index % 2 == 1 { "SHARED.BIN" } else { "shared.bin" }, "byteLength": 36 })).collect::<Vec<_>>(),
			"bufferViews": [{ "buffer": 0, "byteLength": 36 }],
			"accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [0,0,0] }],
			"meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }]
		});
		fs::write(root.join("model.gltf"), model.to_string()).unwrap();
		fs::write(root.join("shared.bin"), [0u8; 36]).unwrap();
		let mut closure = Closure {
			root: &root,
			project: &project,
			catalog: AssetCatalog::default(),
			files: BTreeMap::new(),
			ids: HashSet::new(),
			models: HashMap::new(),
			resolved_paths: HashMap::new(),
			bytes: 0,
			depth: 0,
			uri_edges: 0,
			file_reads: 0,
		};
		closure.model(Path::new("model.gltf"), None).unwrap();
		#[cfg(windows)]
		closure.model(Path::new("MODEL.GLTF"), None).unwrap();
		assert_eq!(closure.uri_edges, 128);
		assert_eq!(
			closure.file_reads, 2,
			"one model read and one shared dependency read"
		);
		assert_eq!(closure.files.len(), 2);
		assert_eq!(
			closure.models.len(),
			1,
			"case aliases reuse one parsed model"
		);
		let prepared = PreparedProject {
			project: project.clone(),
			asset_root: root.clone(),
			files: closure.files.into_values().collect(),
			total_bytes: closure.bytes,
			catalog_source: None,
			catalog_output: None,
		};
		fs::write(root.join("shared.bin"), [1u8; 36]).unwrap();
		assert!(
			prepared
				.validate_inputs()
				.unwrap_err()
				.contains("shared.bin")
		);
		assert!(stage_prepared_project(&prepared, root.to_str().unwrap()).is_err());
		assert!(!root.join(&prepared.project.name).exists());
		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn uri_work_budget_rejects_before_reading_another_dependency() {
		let project = ProjectData::load(
			Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/project.json"),
		)
		.unwrap();
		let mut closure = Closure {
			root: Path::new(env!("CARGO_MANIFEST_DIR")),
			project: &project,
			catalog: AssetCatalog::default(),
			files: BTreeMap::new(),
			ids: HashSet::new(),
			models: HashMap::new(),
			resolved_paths: HashMap::new(),
			bytes: 0,
			depth: 0,
			uri_edges: MAX_URI_EDGES,
			file_reads: 0,
		};
		assert!(
			closure
				.capture_uri(Path::new("model.gltf"), "shared.bin")
				.unwrap_err()
				.contains("100000 URI edges")
		);
		assert_eq!(closure.file_reads, 0);
		assert!(closure.files.is_empty());
	}
}

#[cfg(test)]
mod discovery_tests {
	use super::*;
	struct Fixture {
		parent: PathBuf,
		root: PathBuf,
		project: ProjectData,
		scene: SceneData,
	}
	impl Fixture {
		fn new() -> Self {
			let parent = std::env::temp_dir().join(format!(
				"legacy-prefab-discovery-{}",
				AssetId::generate().as_str()
			));
			fs::create_dir_all(parent.join("assets/scenes")).unwrap();
			let root = parent.join("assets").canonicalize().unwrap();
			let mut scene =
				SceneData::from_json(include_str!("../../apps/game/assets/scenes/robots.json"))
					.unwrap();
			scene.entities.clear();
			scene.bindings.clear();
			scene.camera.follow = None;
			fs::write(root.join("scenes/main.json"), scene.to_json().unwrap()).unwrap();
			let project =
				ProjectData::for_scene("Legacy prefab discovery", "scenes/main.json", &scene)
					.unwrap();
			Self {
				parent,
				root,
				project,
				scene,
			}
		}
	}
	impl Drop for Fixture {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.parent);
		}
	}
	#[test]
	fn uncataloged_prefabs_anywhere_in_assets_stage_with_their_model_dependencies() {
		let fixture = Fixture::new();
		fs::create_dir(fixture.root.join("models")).unwrap();
		fs::create_dir_all(fixture.root.join("old-assemblies/deep")).unwrap();
		let model =
			Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/game/assets/props/cube.glb");
		fs::copy(&model, fixture.root.join("models/prefab-only.glb")).unwrap();
		fs::copy(&model, fixture.root.join("models/unreferenced.glb")).unwrap();
		let prefab = PrefabData {
			schema_version: 1,
			entities: vec![EntityData::new(
				engine_core::EntityKey::new("legacy").unwrap(),
				"Legacy cube",
				"models/prefab-only.glb",
			)],
			gameplay: None,
		};
		let source = prefab.to_json().unwrap();
		for path in ["legacy-root.json", "old-assemblies/deep/assembly.json"] {
			fs::write(fixture.root.join(path), &source).unwrap();
		}
		fs::write(
			fixture.root.join("other-scene.json"),
			fixture.scene.to_json().unwrap(),
		)
		.unwrap();
		fs::write(
			fixture.root.join("metadata.json"),
			r#"{"note":"not a prefab","entities":[]}"#,
		)
		.unwrap();
		let prepared = prepare_project(&fixture.project, &fixture.root).unwrap();
		let staged = stage_prepared_project(&prepared, fixture.parent.to_str().unwrap()).unwrap();
		let output = staged.path().join("assets");
		for path in ["legacy-root.json", "old-assemblies/deep/assembly.json"] {
			assert_eq!(fs::read(output.join(path)).unwrap(), source.as_bytes());
		}
		assert!(output.join("models/prefab-only.glb").is_file());
		assert!(output.join("scenes/main.json").is_file());
		assert!(!output.join("models/unreferenced.glb").exists());
		assert!(!output.join("metadata.json").exists());
		assert!(!output.join("other-scene.json").exists());
		assert!(!fixture.root.join(CATALOG_FILE).exists());
	}
	#[test]
	fn global_discovery_charges_unknown_json_and_skips_registered_scene_documents() {
		let fixture = Fixture::new();
		for (name, bytes) in [
			("ordinary.json", b"{}".as_slice()),
			("malformed.json", b"no".as_slice()),
			("other.json", b"[]".as_slice()),
		] {
			fs::write(fixture.root.join(name), bytes).unwrap();
		}
		assert!(
			prefab_paths_with_budget(&fixture.root, &fixture.project, 5)
				.unwrap_err()
				.contains("JSON byte budget")
		);
		assert!(
			prefab_paths_with_budget(&fixture.root, &fixture.project, 6)
				.unwrap()
				.is_empty()
		);
	}
	#[test]
	fn recognized_invalid_legacy_assemblies_are_reported_instead_of_silently_omitted() {
		let fixture = Fixture::new();
		fs::write(
			fixture.root.join("invalid-assembly.json"),
			r#"{"schema_version":1,"entities":[]}"#,
		)
		.unwrap();
		let error = prepare_project(&fixture.project, &fixture.root)
			.err()
			.unwrap();
		assert!(error.contains("invalid-assembly.json"), "{error}");
	}
}
