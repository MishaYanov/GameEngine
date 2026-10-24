use std::{
	collections::{HashSet, hash_map::DefaultHasher},
	fs,
	hash::Hasher,
	io::{self, Read, Write},
	path::{Path, PathBuf},
	process::Command,
};

use engine_core::{
	project::{BehaviorSpace, BehaviorSpec, ProjectData},
	scene_catalog::SceneDescriptor,
	scene_data::SceneData,
};

struct PackageAsset {
	relative: PathBuf,
	bytes: u64,
	fingerprint: u64,
}

/// Validated authoring inputs. The manifest is captured before a build and
/// checked again while copying, rejecting changed assets instead of mixing eras.
pub struct PreparedProject {
	project: ProjectData,
	asset_root: PathBuf,
	files: Vec<PackageAsset>,
	total_bytes: u64,
}
impl PreparedProject {
	pub fn project(&self) -> &ProjectData {
		&self.project
	}
	pub fn asset_root(&self) -> &Path {
		&self.asset_root
	}
	pub fn asset_count(&self) -> usize {
		self.files.len()
	}
	pub fn total_bytes(&self) -> u64 {
		self.total_bytes
	}
	fn validate_inputs(&self) -> Result<(), String> {
		for asset in &self.files {
			checked_asset_path(&self.asset_root, &asset.relative)
				.and_then(|input| copy_verified_asset(&input, asset, &mut io::sink()))
				.map_err(|cause| {
					format!(
						"Prepared asset {:?} is no longer valid: {cause}",
						asset.relative
					)
				})?;
		}
		Ok(())
	}
}

const MAX_SCENE_REPORT_BYTES: usize = 16 * 1024;
const OMISSION_NOTICE: &str = "\nFurther scene diagnostics omitted.";

pub fn prepare_project(
	project: &ProjectData,
	asset_root: &Path,
) -> Result<PreparedProject, String> {
	project.validate()?;
	let metadata = fs::symlink_metadata(asset_root).map_err(|e| e.to_string())?;
	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err("Asset root must be a directory without symbolic links".into());
	}
	let asset_root = asset_root.canonicalize().map_err(|e| e.to_string())?;
	let files = prepare_assets(&asset_root).map_err(|e| format!("Asset preflight failed: {e}"))?;

	validate_catalog_scenes(project, &asset_root, &files)?;

	let total_bytes = files.iter().map(|entry| entry.bytes).sum();
	Ok(PreparedProject {
		project: project.clone(),
		asset_root,
		files,
		total_bytes,
	})
}

pub fn project_path(name: &str, destination: &str) -> Result<PathBuf, String> {
	let name = name.trim();
	if name.is_empty()
		|| name.len() > 80
		|| !name
			.chars()
			.all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ')
		|| name.ends_with(' ')
	{
		return Err(
			"Use a project name of 1–80 letters, numbers, spaces, hyphens or underscores.".into(),
		);
	}
	let upper = name.to_uppercase();
	if [
		"CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
		"COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
	]
	.contains(&upper.as_str())
	{
		return Err("Choose a name that is not a reserved Windows filename.".into());
	}
	let destination = Path::new(destination.trim());
	if !destination.is_absolute() || !destination.is_dir() {
		return Err("Destination must be an existing absolute folder path.".into());
	}
	let path = destination.join(name);
	if path.exists() {
		return Err("A folder with this project name already exists. Choose another name.".into());
	}
	Ok(path)
}

/// Compatibility entry point for the bundled starter. New projects should use
/// `install_authored_project` with their authored manifest and asset root.
pub fn install_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	project_path(name, destination)?;
	let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.ok_or("Missing engine workspace")?;
	let root = workspace.join("apps/game/assets");
	let manifest = workspace.join("apps/game/project.json");
	let mut project = if manifest.is_file() {
		ProjectData::load(manifest)?
	} else {
		// An explicit application migration keeps historical starter behavior
		// out of the reusable engine's project and simulation modules.
		let scene = read_authored_scene(&root, "scenes/robots.json")?;
		legacy_starter_project(name.trim(), &scene)?
	};
	project.name = name.trim().to_owned();
	install_authored_project(&project, &root, destination)
}

pub fn install_project_from_manifest(
	manifest_path: &Path,
	destination: &str,
) -> Result<PathBuf, String> {
	let project = ProjectData::load(manifest_path)?;
	let root = manifest_path
		.parent()
		.ok_or("Project manifest has no parent folder")?
		.join("assets");
	install_authored_project(&project, &root, destination)
}

pub fn install_authored_project(
	project: &ProjectData,
	asset_root: &Path,
	destination: &str,
) -> Result<PathBuf, String> {
	project_path(&project.name, destination)?;
	let prepared = prepare_project(project, asset_root)?;
	install_prepared_project(&prepared, destination)
}

pub fn install_prepared_project(
	prepared: &PreparedProject,
	destination: &str,
) -> Result<PathBuf, String> {
	let path = project_path(&prepared.project.name, destination)?;
	prepared.validate_inputs()?;
	let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.ok_or("Missing engine workspace")?;
	let target = workspace.join("target");
	let output = Command::new("cargo")
		.current_dir(workspace)
		.args([
			"build",
			"--locked",
			"--package",
			"game",
			"--release",
			"--target-dir",
		])
		.arg(&target)
		.output()
		.map_err(|e| format!("Could not start Cargo: {e}"))?;
	if !output.status.success() {
		return Err(format!(
			"Game build failed:\n{}",
			String::from_utf8_lossy(&output.stderr)
		));
	}
	// create_dir is atomic: never merge into an existing project.
	fs::create_dir(&path).map_err(|e| format!("Could not create project folder: {e}"))?;
	let result = (|| -> Result<(), std::io::Error> {
		fs::copy(
			target
				.join("release")
				.join(format!("game{}", std::env::consts::EXE_SUFFIX)),
			path.join(format!("game{}", std::env::consts::EXE_SUFFIX)),
		)?;
		copy_prepared_assets(&prepared.asset_root, &path.join("assets"), &prepared.files)?;

		let copied_root = path.join("assets").canonicalize()?;

		validate_catalog_scenes(&prepared.project, &copied_root, &prepared.files)
			.map_err(io::Error::other)?;

		let mut project_json = prepared.project.to_json().map_err(io::Error::other)?;
		project_json.push('\n');
		fs::write(path.join("project.json"), project_json)?;
		fs::write(path.join("project-name.txt"), &prepared.project.name)?;
		fs::write(
			path.join("README.txt"),
			"Game Engine project\nRun game.exe to open the Vulkan game window.\nRequires a Vulkan-capable GPU and driver.\nScene JSON and its model, image, material and prefab assets are installed beneath assets.\nAuthoring belongs to the Angular/Tauri editor; keep the assets folder beside the executable.\n",
		)?;
		Ok(())
	})();
	if let Err(error) = result {
		// Retain the new folder for inspection; never recursively remove a path
		// that may have been modified by another process during a long build.
		return Err(format!(
			"Could not install project: {error}; incomplete output retained at {}",
			path.display()
		));
	}
	Ok(path)
}

fn legacy_starter_project(name: &str, scene: &SceneData) -> Result<ProjectData, String> {
	let mut project = ProjectData::for_scene(name, "scenes/robots.json", scene)?;
	if let Some(entity) = scene.bindings.get("robot_a") {
		project.add_behavior(BehaviorSpec::ControlledMove {
			entity: entity.clone(),
			speed: 1.0,
			space: BehaviorSpace::World,
		});
		project.add_behavior(BehaviorSpec::Spin {
			entity: entity.clone(),
			angular_velocity: [0.6, 0.9, 0.0],
		});
	}
	if let Some(entity) = scene.bindings.get("robot_b") {
		project.add_behavior(BehaviorSpec::Spin {
			entity: entity.clone(),
			angular_velocity: [-0.5, 0.75, 0.2],
		});
	}
	project.validate_scene(scene)?;
	Ok(project)
}

fn read_authored_scene(root: &Path, relative: &str) -> Result<SceneData, String> {
	engine_core::scene_data::validate_model_path(relative)?;
	let root = root.canonicalize().map_err(|e| e.to_string())?;
	let path = checked_asset_path(&root, Path::new(relative)).map_err(|e| e.to_string())?;

	read_scene_file(&path)
}

pub fn copy_assets(source: &Path, destination: &Path) -> std::io::Result<()> {
	let source_metadata = fs::symlink_metadata(source)?;
	if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
		return Err(io::Error::other(
			"Asset root must be a directory without symbolic links",
		));
	}
	let root = source.canonicalize()?;
	let files = prepare_assets(&root)?;
	copy_prepared_assets(&root, destination, &files)
}

fn prepare_assets(root: &Path) -> io::Result<Vec<PackageAsset>> {
	let mut files = Vec::new();
	let mut total = 0;
	collect_assets(root, root, 0, &mut files, &mut total, &mut 0)?;
	files.sort_by(|a, b| a.0.cmp(&b.0));
	files
		.into_iter()
		.map(|(relative, bytes)| {
			let path = checked_asset_path(root, &relative)?;
			let fingerprint = copy_and_hash(&path, bytes, &mut io::sink())?;
			Ok(PackageAsset {
				relative,
				bytes,
				fingerprint,
			})
		})
		.collect()
}

fn copy_prepared_assets(root: &Path, destination: &Path, files: &[PackageAsset]) -> io::Result<()> {
	fs::create_dir_all(destination)?;
	for entry in files {
		let input = checked_asset_path(root, &entry.relative)?;
		let output = destination.join(&entry.relative);
		fs::create_dir_all(
			output
				.parent()
				.ok_or_else(|| io::Error::other("Missing asset parent"))?,
		)?;
		let mut writer = fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(output)?;
		copy_verified_asset(&input, entry, &mut writer)?;
	}
	Ok(())
}

fn verify_asset_fingerprint(asset: &PackageAsset, actual: u64) -> io::Result<()> {
	if actual != asset.fingerprint {
		return Err(io::Error::other(
			"Asset changed after preflight; build again from the current authored files",
		));
	}
	Ok(())
}

fn copy_verified_asset(
	input: &Path,
	asset: &PackageAsset,
	writer: &mut impl Write,
) -> io::Result<()> {
	let actual = copy_and_hash(input, asset.bytes, writer)?;
	verify_asset_fingerprint(asset, actual)
}

fn checked_asset_path(root: &Path, relative: &Path) -> io::Result<PathBuf> {
	let mut path = root.to_path_buf();
	for component in relative.components() {
		if !matches!(component, std::path::Component::Normal(_)) {
			return Err(io::Error::other(
				"Asset path must contain only relative normal components",
			));
		}
		path.push(component);
		if fs::symlink_metadata(&path)?.file_type().is_symlink() {
			return Err(io::Error::other("Asset symbolic links are unsupported"));
		}
	}
	let path = path.canonicalize()?;
	if !path.starts_with(root) {
		return Err(io::Error::other("Asset escaped its root"));
	}
	Ok(path)
}

fn copy_and_hash(path: &Path, expected_bytes: u64, writer: &mut impl Write) -> io::Result<u64> {
	let file = fs::File::open(path)?;
	let metadata = file.metadata()?;
	if !metadata.is_file() || metadata.len() != expected_bytes {
		return Err(io::Error::other("Asset changed size after preflight"));
	}
	let mut reader = file.take(expected_bytes + 1);
	let mut buffer = [0u8; 64 * 1024];
	let mut copied = 0u64;
	let mut fingerprint = DefaultHasher::new();
	loop {
		let read = reader.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		copied += read as u64;
		if copied > expected_bytes {
			return Err(io::Error::other("Asset grew during installation"));
		}
		fingerprint.write(&buffer[..read]);
		writer.write_all(&buffer[..read])?;
	}
	if copied != expected_bytes {
		return Err(io::Error::other("Asset shrank during installation"));
	}
	Ok(fingerprint.finish())
}

fn collect_assets(
	root: &Path,
	path: &Path,
	depth: usize,
	files: &mut Vec<(PathBuf, u64)>,
	total: &mut u64,
	visited: &mut usize,
) -> io::Result<()> {
	*visited += 1;
	if depth > 32 || *visited > 10_000 {
		return Err(io::Error::other(
			"Assets exceed 32 directory levels or 10000 entries",
		));
	}
	let metadata = fs::symlink_metadata(path)?;
	if metadata.file_type().is_symlink() || !path.canonicalize()?.starts_with(root) {
		return Err(io::Error::other(
			"Asset symbolic links and paths outside the asset root are unsupported",
		));
	}
	if metadata.is_dir() {
		let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
		entries.sort_by_key(|entry| entry.file_name());
		for entry in entries {
			collect_assets(root, &entry.path(), depth + 1, files, total, visited)?;
		}
		return Ok(());
	}
	if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
		return Err(io::Error::other(
			"Asset must be a regular file of at most 64 MiB",
		));
	}
	let extension = path
		.extension()
		.and_then(|value| value.to_str())
		.unwrap_or("")
		.to_ascii_lowercase();
	if !["png", "jpg", "jpeg", "obj", "gltf", "glb", "bin", "json"].contains(&extension.as_str()) {
		return Err(io::Error::other("Unsupported packaged asset format"));
	}
	if extension == "json" && metadata.len() > 16 * 1024 * 1024 {
		return Err(io::Error::other("Asset JSON exceeds 16 MiB"));
	}
	*total = total
		.checked_add(metadata.len())
		.ok_or_else(|| io::Error::other("Asset byte count overflow"))?;
	if *total > 512 * 1024 * 1024 {
		return Err(io::Error::other("Assets exceed 512 MiB"));
	}
	let relative = path.strip_prefix(root).map_err(io::Error::other)?;
	if relative.as_os_str().len() > 1024 {
		return Err(io::Error::other("Asset path exceeds 1024 characters"));
	}
	files.push((relative.to_path_buf(), metadata.len()));
	Ok(())
}

fn scene_error(entry: &SceneDescriptor, cause: String) -> String {
	format!(
		"Scene {:?} ({:?}): {cause}",
		entry.id().as_str(),
		entry.asset_path()
	)
}

fn validate_catalog_scenes(
	project: &ProjectData,
	root: &Path,
	files: &[PackageAsset],
) -> Result<(), String> {
	let packaged: HashSet<&Path> = files.iter().map(|asset| asset.relative.as_path()).collect();

	let mut report = String::new();
	let mut truncated = false;

	for entry in ordered_scenes(project) {
		if let Err(error) = validate_catalog_scene(project, root, entry, &packaged) {
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

	for entity in scene.entities.iter().filter(|entity| entity.renderable) {
		if !checked.insert(entity.model_path.as_str()) {
			continue;
		}

		let relative = Path::new(&entity.model_path);
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
	project: &ProjectData,
	root: &Path,
	entry: &SceneDescriptor,
	packaged: &HashSet<&Path>,
) -> Result<(), String> {
	let path = prepared_asset_path(root, Path::new(entry.asset_path()), packaged)
		.map_err(|cause| scene_error(entry, cause))?;
	let scene = read_scene_file(&path).map_err(|cause| scene_error(entry, cause))?;

	project
		.validate_scene_for(entry.id(), &scene)
		.map_err(|cause| scene_error(entry, cause))?;

	validate_model_references(&scene, root, packaged).map_err(|cause| scene_error(entry, cause))?;

	Ok(())
}

fn ordered_scenes(project: &ProjectData) -> Vec<&SceneDescriptor> {
	let mut scenes: Vec<_> = project.scenes.iter().collect();

	scenes.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));

	scenes
}

fn append_scene_error(report: &mut String, error: &str, truncated: &mut bool) {
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

fn prepared_asset_path(
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

fn read_scene_file(path: &Path) -> Result<SceneData, String> {
	// Read and parse the already-resolved file.
	let file = fs::File::open(path).map_err(|e| e.to_string())?;
	let mut json = String::new();

	file.take(engine_core::scene_data::MAX_SCENE_BYTES as u64 + 1)
		.read_to_string(&mut json)
		.map_err(|e| e.to_string())?;

	engine_core::scene_data::parse_json(&json).map(|parsed| parsed.data)
}

#[cfg(test)]
#[path = "tests_lib.rs"]
mod tests;
