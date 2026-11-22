//! Bounded authoring asset transactions. Preparation never replaces live files.
//! Publication is failure-atomic (not a crash-atomic multi-file database).
use engine_core::{
	asset_catalog::{
		AssetCatalog, AssetId, AssetKind, CATALOG_FILE, MAX_CATALOG_BYTES, MeshSignature,
		PersistentAsset,
	},
	asset_registry::AssetRef,
	scene_data::validate_model_path,
};
use sha2::{Digest, Sha256};
use std::{
	collections::HashSet,
	fs::{self, OpenOptions},
	io::{Read, Write},
	path::{Path, PathBuf},
};

mod animation_clip;
mod input_actions;
mod migration;
mod prefab;

pub use animation_clip::{prepare_animation_clip, read_animation_clip};
pub use input_actions::{active_input_actions, load_input_actions, prepare_input_actions};
pub use migration::migrate_catalog;
use prefab::prefab_dependencies;

const MAX_SOURCE: usize = 64 * 1024 * 1024;
pub fn fingerprint(bytes: &[u8]) -> String {
	format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
	let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
	if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit as u64 {
		return Err("Asset must be a regular file within its byte limit".into());
	}
	let mut bytes = Vec::new();
	fs::File::open(path)
		.map_err(|e| e.to_string())?
		.take(limit as u64 + 1)
		.read_to_end(&mut bytes)
		.map_err(|e| e.to_string())?;
	if bytes.len() > limit {
		return Err("Asset grew beyond its byte limit".into());
	}
	Ok(bytes)
}
fn root(path: &Path) -> Result<PathBuf, String> {
	let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
	if !metadata.is_dir() || metadata.file_type().is_symlink() {
		return Err("Asset root must be a directory without symbolic links".into());
	}
	path.canonicalize().map_err(|e| e.to_string())
}
/// Validate each existing path component, including the final leaf. New targets
/// require an existing parent; imports never create an arbitrary directory tree.
pub fn asset_path(asset_root: &Path, relative: &str, must_exist: bool) -> Result<PathBuf, String> {
	validate_model_path(relative)?;
	let asset_root = root(asset_root)?;
	let mut path = asset_root.clone();
	for component in Path::new(relative).components() {
		path.push(component);
		if let Ok(metadata) = fs::symlink_metadata(&path)
			&& metadata.file_type().is_symlink()
		{
			return Err("Asset paths must not contain symbolic links".into());
		}
	}
	let parent = path
		.parent()
		.ok_or("Asset needs a parent")?
		.canonicalize()
		.map_err(|e| e.to_string())?;
	if !parent.starts_with(&asset_root) {
		return Err("Asset path escapes its root".into());
	}
	if must_exist {
		let resolved = path.canonicalize().map_err(|e| e.to_string())?;
		if !resolved.starts_with(&asset_root) {
			return Err("Asset path escapes its root".into());
		}
		Ok(resolved)
	} else {
		Ok(path)
	}
}
fn catalog_bytes(asset_root: &Path) -> Result<Option<Vec<u8>>, String> {
	let path = asset_root.join(CATALOG_FILE);
	match fs::symlink_metadata(&path) {
		Ok(_) => read(&path, MAX_CATALOG_BYTES).map(Some),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(e.to_string()),
	}
}
pub fn load_catalog(asset_root: &Path) -> Result<AssetCatalog, String> {
	let asset_root = root(asset_root)?;
	match catalog_bytes(&asset_root)? {
		Some(bytes) => {
			AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
		}
		None => Ok(AssetCatalog::default()),
	}
}
pub(super) fn warm_model_path(asset_root: &Path, relative: &str) -> Result<PathBuf, String> {
	validate_model_path(relative)?;
	let asset_root = root(asset_root)?;
	crate::scene_io::paths::resolve_from_canonical_root(&asset_root, Path::new(relative))
}

pub(super) fn open_warm_file(path: &Path) -> std::io::Result<fs::File> {
	use crate::scene_io::paths::is_linked;
	if is_linked(&fs::symlink_metadata(path)?) {
		return Err(std::io::Error::other(
			"Warm source must be an ordinary file",
		));
	}
	let mut options = OpenOptions::new();
	options.read(true);
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
	}
	#[cfg(windows)]
	{
		use std::os::windows::fs::OpenOptionsExt;
		use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
		options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
	}
	let file = options.open(path)?;
	let metadata = file.metadata()?;
	if !metadata.is_file() || is_linked(&metadata) {
		return Err(std::io::Error::other(
			"Warm source must be an ordinary file",
		));
	}
	Ok(file)
}

pub(super) fn load_catalog_for_warming(
	asset_root: &Path,
	budget: &mut super::import_budget::ImportBudget<'_>,
) -> Result<AssetCatalog, String> {
	let path = warm_model_path(asset_root, CATALOG_FILE)?;
	let bytes = budget
		.read_file_at_most(&path, MAX_CATALOG_BYTES)
		.map_err(|e| e.to_string())?;
	budget.preflight_json(&bytes).map_err(|e| e.to_string())?;
	AssetCatalog::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
}

pub fn resolve_asset(asset_root: &Path, reference: &AssetRef) -> Result<AssetRef, String> {
	resolve_asset_with_model(asset_root, reference, |_| ()).map(|(reference, ())| reference)
}

pub(super) fn resolve_asset_with_model<T>(
	asset_root: &Path,
	reference: &AssetRef,
	use_model: impl FnOnce(Option<&PersistentAsset>) -> T,
) -> Result<(AssetRef, T), String> {
	reference.validate()?;
	let catalog = load_catalog(asset_root)?;
	match catalog.resolve(
		reference.asset_id.as_ref(),
		&reference.path,
		reference.mesh_index,
	)? {
		Some(asset) if asset.kind == AssetKind::Model => Ok((
			AssetRef::new(&asset.path, reference.mesh_index)?.with_asset_id(Some(asset.id.clone())),
			use_model(Some(asset)),
		)),
		Some(_) => Err("A renderable entity requires a model asset".into()),
		None => Ok((reference.clone(), use_model(None))),
	}
}
fn unique(asset_root: &Path, suffix: &str) -> PathBuf {
	asset_root.join(format!(
		".engine-asset-{}.{suffix}",
		AssetId::generate().as_str()
	))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
	let mut file = OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)
		.map_err(|e| e.to_string())?;
	let result = file.write_all(bytes).and_then(|_| file.sync_all());
	drop(file);
	if let Err(error) = result {
		let _ = fs::remove_file(path);
		return Err(error.to_string());
	}
	Ok(())
}
pub(crate) fn install_new(source: &Path, target: &Path) -> Result<(), String> {
	// MoveFileExW without REPLACE_EXISTING publishes exclusively on volumes that
	// do not support hard links. std::fs::rename may replace an existing target.
	#[cfg(windows)]
	{
		use std::os::windows::ffi::OsStrExt;
		let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
		let target: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
		let result = unsafe {
			windows_sys::Win32::Storage::FileSystem::MoveFileExW(
				source.as_ptr(),
				target.as_ptr(),
				0,
			)
		};
		if result == 0 {
			Err(std::io::Error::last_os_error().to_string())
		} else {
			Ok(())
		}
	}
	#[cfg(not(windows))]
	{
		fs::hard_link(source, target).map_err(|e| e.to_string())
	}
}
fn signatures(
	path: &Path,
	asset_root: &Path,
	self_contained: bool,
) -> Result<Vec<MeshSignature>, String> {
	let bytes = read(path, MAX_SOURCE)?;
	if self_contained && !bytes.starts_with(b"glTF") {
		return Err("External imports require a binary .glb file".into());
	}
	let mut budget = super::import_budget::ImportBudget::new(None);
	if bytes.starts_with(b"glTF") {
		let binary = gltf::binary::Glb::from_slice(&bytes).map_err(|e| e.to_string())?;
		budget
			.preflight_json(binary.json.as_ref())
			.map_err(|e| e.to_string())?;
	} else {
		budget.preflight_json(&bytes).map_err(|e| e.to_string())?;
	}
	let parsed = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
	if parsed.skins().len() > 0 {
		// The shared CPU decoder owns skin/clip validation; the static scene
		// importer deliberately rejects skins and is not used for this path.
		bridge::animation_asset::decode(&bytes)?;
	}
	if self_contained
		&& (parsed.buffers().any(
			|b| matches!(b.source(),gltf::buffer::Source::Uri(uri) if !uri.starts_with("data:")),
		) || parsed.images().any(
			|i| matches!(i.source(),gltf::image::Source::Uri{uri,..} if !uri.starts_with("data:")),
		)) {
		return Err("External GLB imports must embed their buffers and images".into());
	}
	let meshes = parsed
		.meshes()
		.map(|mesh| MeshSignature {
			index: mesh.index(),
			name: mesh.name().unwrap_or("Mesh").to_owned(),
			primitives: mesh.primitives().len(),
		})
		.collect::<Vec<_>>();
	if meshes.is_empty() || meshes.len() > 10_000 {
		return Err("Model requires 1–10000 meshes".into());
	}
	super::gltf_loader::preflight_meshes(&parsed.document, meshes.iter().map(|mesh| mesh.index))
		.map_err(|e| e.to_string())?;
	// Validate every mesh/image, including unreferenced meshes, before publishing.
	let mut resources =
		super::gltf_loader::load_resources_in_root(path, asset_root).map_err(|e| e.to_string())?;
	resources.prepare_all_images().map_err(|e| e.to_string())?;
	for mesh in &meshes {
		resources
			.import_mesh(mesh.index)
			.map_err(|e| e.to_string())?;
	}
	Ok(meshes)
}
pub struct PreparedAssetChange {
	root: PathBuf,
	operation_id: String,
	candidate: PathBuf,
	destination: String,
	original_catalog: Option<Vec<u8>>,
	original_asset: Option<(PathBuf, String)>,
	catalog: AssetCatalog,
	asset: PersistentAsset,
	reimport: bool,
}
impl PreparedAssetChange {
	/// Captured disk state to compare with an editor draft opened earlier.
	pub fn previous_fingerprint(&self) -> Option<&str> {
		self.original_asset
			.as_ref()
			.map(|(_, fingerprint)| fingerprint.as_str())
	}
	pub fn operation_id(&self) -> &str {
		&self.operation_id
	}
	pub fn candidate_path(&self) -> &Path {
		&self.candidate
	}
	pub fn destination_path(&self) -> &str {
		&self.destination
	}
	pub fn asset(&self) -> &PersistentAsset {
		&self.asset
	}
	pub fn is_reimport(&self) -> bool {
		self.reimport
	}
	/// Check again under the host workspace lock, after CPU/GPU preparation.
	pub fn validate_current(&self) -> Result<(), String> {
		if catalog_bytes(&self.root)? != self.original_catalog {
			return Err("Asset catalog changed while preparing the operation".into());
		}
		if let Some((path, expected)) = &self.original_asset
			&& fingerprint(&read(path, MAX_SOURCE)?) != *expected
		{
			return Err("Asset changed while preparing the operation".into());
		}
		if fingerprint(&read(&self.candidate, MAX_SOURCE)?) != self.asset.fingerprint {
			return Err("Prepared asset changed before publication".into());
		}
		let destination = asset_path(&self.root, &self.destination, false)?;
		if !self.reimport && destination.exists() {
			return Err("Asset destination already exists".into());
		}
		Ok(())
	}
	pub fn publish(self) -> Result<PersistentAsset, String> {
		self.publish_with(install_new)
	}
	fn publish_with(
		self,
		mut install: impl FnMut(&Path, &Path) -> Result<(), String>,
	) -> Result<PersistentAsset, String> {
		self.validate_current()?;
		let target = asset_path(&self.root, &self.destination, false)?;
		let catalog_path = self.root.join(CATALOG_FILE);
		let catalog_temp = unique(&self.root, "catalog.tmp");
		write_new(&catalog_temp, self.catalog.to_json()?.as_bytes())?;
		let backup = unique(&self.root, "previous.tmp");
		let catalog_backup = unique(&self.root, "catalog-previous.tmp");
		let mut target_saved = false;
		let mut target_installed = false;
		let mut catalog_saved = false;
		let mut catalog_installed = false;
		let result = (|| {
			// Move retains the old path until both publications have succeeded.
			if self.reimport {
				fs::rename(&target, &backup).map_err(|e| e.to_string())?;
				target_saved = true;
				let expected = &self
					.original_asset
					.as_ref()
					.ok_or("Replacement source is unavailable")?
					.1;
				if fingerprint(&read(&backup, MAX_SOURCE)?) != *expected {
					return Err("Asset changed at the publication boundary".into());
				}
			}
			// Exclusive publication never replaces a concurrently created target.
			install(&self.candidate, &target)?;
			target_installed = true;
			if let Some(expected) = &self.original_catalog {
				fs::rename(&catalog_path, &catalog_backup).map_err(|e| e.to_string())?;
				catalog_saved = true;
				if read(&catalog_backup, MAX_CATALOG_BYTES)? != *expected {
					return Err("Asset catalog changed at the publication boundary".into());
				}
			}
			install(&catalog_temp, &catalog_path)?;
			catalog_installed = true;
			if !self.reimport
				&& let Some((old, expected)) = &self.original_asset
			{
				if fingerprint(&read(old, MAX_SOURCE)?) != *expected {
					return Err("Moved asset changed at the publication boundary".into());
				}
				fs::remove_file(old).map_err(|e| e.to_string())?;
			}
			Ok::<_, String>(())
		})();
		if let Err(error) = result {
			let mut failures = Vec::new();
			if catalog_installed && let Err(e) = fs::remove_file(&catalog_path) {
				failures.push(e.to_string());
			}
			if catalog_saved && let Err(e) = install_new(&catalog_backup, &catalog_path) {
				failures.push(e);
			}
			if target_installed && let Err(e) = fs::remove_file(&target) {
				failures.push(e.to_string());
			}
			if target_saved && let Err(e) = install_new(&backup, &target) {
				failures.push(e);
			}
			let _ = fs::remove_file(&catalog_temp);
			return Err(if failures.is_empty() {
				error
			} else {
				format!(
					"{error}; rollback needs recovery (asset backup {}, catalog backup {}): {}",
					backup.display(),
					catalog_backup.display(),
					failures.join("; ")
				)
			});
		}
		let _ = fs::remove_file(&catalog_temp);
		let _ = fs::remove_file(&catalog_backup);
		let _ = fs::remove_file(&backup);
		Ok(self.asset.clone())
	}
}
impl Drop for PreparedAssetChange {
	fn drop(&mut self) {
		let _ = fs::remove_file(&self.candidate);
	}
}
fn prepare(
	asset_root: &Path,
	source: &Path,
	destination: &str,
	previous: Option<&PersistentAsset>,
	move_asset: bool,
	used: &HashSet<usize>,
) -> Result<PreparedAssetChange, String> {
	let asset_root = root(asset_root)?;
	validate_model_path(destination)?;
	if !destination.to_ascii_lowercase().ends_with(".glb") && !move_asset {
		return Err("Imported model destination must use .glb".into());
	}
	let original_catalog = catalog_bytes(&asset_root)?;
	let mut catalog = load_catalog(&asset_root)?;
	if catalog
		.by_path(destination)
		.is_some_and(|asset| previous.is_none_or(|old| old.id != asset.id) || move_asset)
	{
		return Err("Destination is reserved by an asset or historical alias".into());
	}
	let target = asset_path(&asset_root, destination, false)?;
	if (previous.is_none() || move_asset) && target.exists() {
		return Err("Asset destination already exists".into());
	}
	let bytes = read(source, MAX_SOURCE)?;
	let candidate = unique(&asset_root, "candidate.tmp");
	write_new(&candidate, &bytes)?;
	let result = (|| {
		let meshes = if move_asset {
			previous
				.ok_or("Move requires an existing asset")?
				.meshes
				.clone()
		} else {
			signatures(&candidate, &asset_root, true)?
		};
		let mut asset = PersistentAsset {
			id: previous.map_or_else(AssetId::generate, |old| old.id.clone()),
			path: destination.into(),
			kind: previous.map_or(AssetKind::Model, |old| old.kind),
			aliases: previous.map_or_else(Vec::new, |old| old.aliases.clone()),
			revision: previous.map_or(Ok(1), |old| {
				old.revision
					.checked_add(1)
					.ok_or("Asset revision exhausted")
			})?,
			fingerprint: fingerprint(&bytes),
			bytes: bytes.len() as u64,
			source_path: Some(source.to_string_lossy().into_owned()),
			meshes,
			dependencies: previous.map_or_else(Vec::new, |old| old.dependencies.clone()),
		};
		let original_asset = previous
			.map(|old| {
				Ok::<_, String>((
					asset_path(&asset_root, &old.path, true)?,
					old.fingerprint.clone(),
				))
			})
			.transpose()?;
		if let Some(old) = previous {
			if move_asset {
				asset.aliases.push(old.path.clone());
				asset.source_path = old.source_path.clone();
			} else {
				old.check_reimport(&asset, used)?;
			}
		}
		catalog.replace(asset.clone())?;
		Ok(PreparedAssetChange {
			root: asset_root,
			operation_id: AssetId::generate().as_str().into(),
			candidate: candidate.clone(),
			destination: destination.into(),
			original_catalog,
			original_asset,
			catalog,
			asset,
			reimport: previous.is_some() && !move_asset,
		})
	})();
	if result.is_err() {
		let _ = fs::remove_file(candidate);
	}
	result
}
pub fn prepare_import(
	asset_root: &Path,
	source: &Path,
	destination: &str,
) -> Result<PreparedAssetChange, String> {
	prepare(
		asset_root,
		source,
		destination,
		None,
		false,
		&HashSet::new(),
	)
}
pub fn prepare_reimport(
	asset_root: &Path,
	id: &AssetId,
	source: Option<&Path>,
	expected_fingerprint: &str,
	used_meshes: &HashSet<usize>,
) -> Result<PreparedAssetChange, String> {
	let catalog = load_catalog(asset_root)?;
	let old = catalog.by_id(id).ok_or("Asset identity is unavailable")?;
	if old.kind != AssetKind::Model || old.fingerprint != expected_fingerprint {
		return Err("Asset fingerprint changed; refresh before reimporting".into());
	}
	let source = source
		.or_else(|| old.source_path.as_deref().map(Path::new))
		.ok_or("Choose a reimport source")?;
	prepare(asset_root, source, &old.path, Some(old), false, used_meshes)
}
pub fn prepare_move(
	asset_root: &Path,
	id: &AssetId,
	destination: &str,
) -> Result<PreparedAssetChange, String> {
	let catalog = load_catalog(asset_root)?;
	let old = catalog.by_id(id).ok_or("Asset identity is unavailable")?;
	// External-buffer .gltf moves require rewriting dependencies. Keep supported
	// authoring moves explicit and safe: self-contained imported GLBs only.
	let source = asset_path(asset_root, &old.path, true)?;
	match old.kind {
		AssetKind::Model => {
			if !old.path.to_ascii_lowercase().ends_with(".glb")
				|| !destination.to_ascii_lowercase().ends_with(".glb")
			{
				return Err(
					"Supported model moves require self-contained .glb source and destination"
						.into(),
				);
			}
			signatures(&source, asset_root, true)?;
		}
		AssetKind::Prefab => {
			if !destination.to_ascii_lowercase().ends_with(".json") {
				return Err("Prefab files use .json".into());
			}
			engine_core::editor::PrefabData::from_json(
				std::str::from_utf8(&read(
					&source,
					engine_core::editor::PrefabData::MAX_JSON_BYTES,
				)?)
				.map_err(|e| e.to_string())?,
			)?;
		}
		AssetKind::InputActions => {
			if !destination
				.to_ascii_lowercase()
				.ends_with(engine_core::input::INPUT_ACTION_MAP_SUFFIX)
			{
				return Err("Input Actions assets use .input-actions.json".into());
			}
			load_input_actions(asset_root, old)?;
		}
		_ => return Err("This asset kind does not support moving".into()),
	}
	prepare(
		asset_root,
		&source,
		destination,
		Some(old),
		true,
		&HashSet::new(),
	)
}
pub fn prepare_prefab(
	asset_root: &Path,
	destination: &str,
	prefab: &engine_core::editor::PrefabData,
) -> Result<PreparedAssetChange, String> {
	let asset_root = root(asset_root)?;
	if !destination.to_ascii_lowercase().ends_with(".json") {
		return Err("Prefab files use .json".into());
	}
	let target = asset_path(&asset_root, destination, false)?;
	let original_catalog = catalog_bytes(&asset_root)?;
	let mut catalog = load_catalog(&asset_root)?;
	let old = catalog.by_path(destination).cloned();
	if old
		.as_ref()
		.is_some_and(|asset| asset.path != destination || asset.kind != AssetKind::Prefab)
	{
		return Err("Prefab destination conflicts with a reserved asset path".into());
	}
	let original_asset = if target.exists() {
		let bytes = read(&target, engine_core::editor::PrefabData::MAX_JSON_BYTES)?;
		if old.is_none() {
			engine_core::editor::PrefabData::from_json(
				std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
			)?;
		}
		Some((target, fingerprint(&bytes)))
	} else {
		None
	};
	let owner = old
		.as_ref()
		.map_or_else(AssetId::generate, |asset| asset.id.clone());
	if !prefab.composition.is_empty() {
		prefab
			.composition
			.validate(Some(&owner), &prefab.entities, &prefab.reference_slots)?;
	}
	// Enroll legacy model references in this same catalog transaction; saving a
	// prefab must not publish an incomplete dependency graph or rewrite the scene.
	for entity in prefab.expanded_entities()?.iter() {
		if catalog
			.resolve(
				entity.asset_id.as_ref(),
				&entity.model_path,
				entity.mesh_index,
			)?
			.is_some()
		{
			continue;
		}
		if !entity.renderable && !asset_path(&asset_root, &entity.model_path, false)?.exists() {
			continue;
		}
		let path = asset_path(&asset_root, &entity.model_path, true)?;
		let bytes = read(&path, MAX_SOURCE)?;
		let meshes = signatures(&path, &asset_root, false)?;
		catalog.replace(PersistentAsset {
			id: AssetId::generate(),
			path: entity.model_path.clone(),
			kind: AssetKind::Model,
			aliases: Vec::new(),
			revision: 1,
			fingerprint: fingerprint(&bytes),
			bytes: bytes.len() as u64,
			source_path: None,
			meshes,
			dependencies: Vec::new(),
		})?;
	}
	let bytes = prefab.to_json()?.into_bytes();
	let asset = PersistentAsset {
		id: owner,
		path: destination.into(),
		kind: AssetKind::Prefab,
		aliases: old
			.as_ref()
			.map_or_else(Vec::new, |asset| asset.aliases.clone()),
		revision: old.as_ref().map_or(Ok(1), |asset| {
			asset
				.revision
				.checked_add(1)
				.ok_or("Asset revision exhausted")
		})?,
		fingerprint: fingerprint(&bytes),
		bytes: bytes.len() as u64,
		source_path: None,
		meshes: Vec::new(),
		dependencies: prefab_dependencies(&catalog, prefab)?,
	};
	catalog.replace(asset.clone())?;
	let candidate = unique(&asset_root, "candidate.tmp");
	write_new(&candidate, &bytes)?;
	Ok(PreparedAssetChange {
		root: asset_root,
		operation_id: AssetId::generate().as_str().into(),
		candidate,
		destination: destination.into(),
		original_catalog,
		reimport: original_asset.is_some(),
		original_asset,
		catalog,
		asset,
	})
}

#[cfg(test)]
#[path = "../../tests/unit/assets/project_assets.rs"]
mod tests;
