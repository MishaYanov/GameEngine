//! Validate scene documents, packaged references, and project behaviors.

use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

use engine_core::{
	game_session::GameSession,
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::SceneData,
};

use super::{
	PackageAsset, Path, PathBuf, PreparedProject, ProjectData, Read, checked_asset_path,
	dependencies, fs,
};

const MAX_SCENE_REPORT_BYTES: usize = 16 * 1024;
pub(super) const OMISSION_NOTICE: &str = "\nFurther scene diagnostics omitted.";

pub fn prepare_project(
	project: &ProjectData,
	asset_root: &Path,
) -> Result<PreparedProject, String> {
	project.validate()?;
	courtyard_gameplay::resolve(project)?;
	let metadata = fs::symlink_metadata(asset_root).map_err(|e| e.to_string())?;
	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err("Asset root must be a directory without symbolic links".into());
	}
	let asset_root = asset_root.canonicalize().map_err(|e| e.to_string())?;
	let dependencies::DependencySnapshot {
		files,
		source_files,
		generated_files,
		catalog_source,
		catalog_output,
	} = dependencies::prepare(project, &asset_root)?;

	validate_catalog_scenes(project, &asset_root, &files)?;

	let total_bytes = files.iter().map(|entry| entry.bytes).sum();
	Ok(PreparedProject {
		project: project.clone(),
		asset_root,
		files,
		source_files,
		generated_files,
		total_bytes,
		catalog_source,
		catalog_output,
	})
}

pub(super) fn read_authored_scene(root: &Path, relative: &str) -> Result<SceneData, String> {
	engine_core::scene_data::validate_model_path(relative)?;
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let path = checked_asset_path(&root, Path::new(relative)).map_err(|e| e.to_string())?;

	read_scene_file(&path)
}

pub(super) fn scene_error(entry: &SceneDescriptor, cause: String) -> String {
	format!(
		"Scene {:?} ({:?}): {cause}",
		entry.id().as_str(),
		entry.asset_path()
	)
}

pub(super) fn validate_catalog_scenes(
	project: &ProjectData,
	root: &Path,
	files: &[PackageAsset],
) -> Result<(), String> {
	// Keep one validated project and target index for this immutable catalog pass.
	project.validate()?;
	let gameplay = (
		Arc::new(project.clone()),
		courtyard_gameplay::resolve(project)?,
	);
	let mut targets: HashMap<&SceneId, Vec<&engine_core::EntityKey>> = HashMap::new();
	for behavior in &project.behaviors {
		targets
			.entry(&behavior.scene_id)
			.or_default()
			.push(behavior.entity());
	}
	let packaged: HashSet<&Path> = files.iter().map(|asset| asset.relative.as_path()).collect();

	let mut report = String::new();
	let mut truncated = false;

	for entry in ordered_scenes(project) {
		let scene_targets = targets.get(entry.id()).map_or(&[][..], Vec::as_slice);
		let result =
			validate_catalog_scene(scene_targets, root, entry, &packaged).and_then(|scene| {
				{
					let (project, factory) = &gameplay;
					let mut game = GameSession::with_factory_for_scene(
						Arc::clone(project),
						entry.id().clone(),
						Arc::new(scene),
						Arc::clone(factory),
					)
					.map_err(|cause| scene_error(entry, cause))?;
					let animation_root = root.to_path_buf();
					game.set_animation_resolver(Arc::new(move |scene| {
						bridge::animation_asset::prepare_scene(&animation_root, scene)
					}))
					.map_err(|cause| scene_error(entry, cause))?;
				}
				Ok(())
			});
		if let Err(error) = result {
			append_scene_error(&mut report, &error, &mut truncated);
		}
	}

	if report.is_empty() {
		Ok(())
	} else {
		Err(report)
	}
}

fn validate_model_references(
	scene: &SceneData,
	root: &Path,
	packaged: &HashSet<&Path>,
) -> Result<(), String> {
	let mut checked = HashSet::new();
	let catalog = dependencies::read_catalog(root)?;

	for entity in scene.entities.iter().filter(|entity| entity.renderable) {
		let resolved = catalog.resolve(
			entity.asset_id.as_ref(),
			&entity.model_path,
			entity.mesh_index,
		)?;
		if resolved.is_some_and(|asset| asset.kind != engine_core::asset_catalog::AssetKind::Model)
		{
			return Err("Renderable entity references a non-model asset identity".into());
		}
		let model_path = resolved.map_or(entity.model_path.as_str(), |asset| asset.path.as_str());
		if !checked.insert(model_path) {
			continue;
		}

		let relative = Path::new(model_path);
		let supported = relative
			.extension()
			.and_then(|value| value.to_str())
			.is_some_and(|extension| {
				extension.eq_ignore_ascii_case("gltf") || extension.eq_ignore_ascii_case("glb")
			});

		if !supported {
			return Err(format!(
				"Renderable model '{}' must use glTF (.gltf or .glb)",
				entity.model_path
			));
		}

		prepared_asset_path(root, relative, packaged)
			.map_err(|error| format!("Model '{}' preflight failed: {error}", entity.model_path))?;
	}

	Ok(())
}

fn validate_catalog_scene(
	targets: &[&engine_core::EntityKey],
	root: &Path,
	entry: &SceneDescriptor,
	packaged: &HashSet<&Path>,
) -> Result<SceneData, String> {
	let path = prepared_asset_path(root, Path::new(entry.asset_path()), packaged)
		.map_err(|cause| scene_error(entry, cause))?;
	let scene = read_scene_file(&path).map_err(|cause| scene_error(entry, cause))?;

	// read_scene_file parses and validates the document before returning it.
	// Only cross-document target membership remains to check here.
	if !targets.is_empty() {
		let keys: HashSet<_> = scene.entities.iter().map(|entity| &entity.key).collect();
		for (index, target) in targets.iter().enumerate() {
			if !keys.contains(target) {
				return Err(scene_error(
					entry,
					format!(
						"Behavior {index} targets missing entity {:?}",
						target.as_str()
					),
				));
			}
		}
	}

	validate_model_references(&scene, root, packaged).map_err(|cause| scene_error(entry, cause))?;

	Ok(scene)
}

pub(super) fn ordered_scenes(project: &ProjectData) -> Vec<&SceneDescriptor> {
	let mut scenes: Vec<_> = project.scenes.iter().collect();

	scenes.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));

	scenes
}

pub(super) fn append_scene_error(report: &mut String, error: &str, truncated: &mut bool) {
	if *truncated {
		return;
	}

	// Keep space for the notice, including its leading newline.
	let content_limit = MAX_SCENE_REPORT_BYTES - OMISSION_NOTICE.len();
	let remaining = content_limit.saturating_sub(report.len());
	let separator = if report.is_empty() { "" } else { "\n" };

	if separator.len() + error.len() <= remaining {
		report.push_str(separator);
		report.push_str(error);
		return;
	}

	// Append as much of this error as fits.
	if remaining >= separator.len() {
		report.push_str(separator);

		let mut end = remaining - separator.len();

		while !error.is_char_boundary(end) {
			end -= 1;
		}

		report.push_str(&error[..end]);
	}

	report.push_str(OMISSION_NOTICE);
	*truncated = true;
}

pub(super) fn prepared_asset_path(
	root: &Path,
	relative: &Path,
	packaged: &HashSet<&Path>,
) -> Result<PathBuf, String> {
	let path = checked_asset_path(root, relative).map_err(|error| error.to_string())?;
	if !fs::metadata(&path)
		.map_err(|error| error.to_string())?
		.is_file()
	{
		return Err("Asset must be a regular file".into());
	}
	let canonical_relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
	if !packaged.contains(canonical_relative) {
		return Err("Asset is absent from the prepared asset snapshot".into());
	}
	Ok(path)
}

pub(super) fn read_scene_file(path: &Path) -> Result<SceneData, String> {
	// Read and parse the already-resolved file.
	let file = fs::File::open(path).map_err(|e| e.to_string())?;
	let mut json = String::new();

	file.take(engine_core::scene_data::MAX_SCENE_BYTES as u64 + 1)
		.read_to_string(&mut json)
		.map_err(|e| e.to_string())?;

	engine_core::scene_data::parse_json(&json).map(|parsed| parsed.data)
}
