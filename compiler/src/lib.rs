use std::{
	collections::{HashMap, HashSet},
	fs,
	io::{self, Read, Write},
	path::{Path, PathBuf},
	process::Command,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
};

use sha2::{Digest, Sha256};

use engine_core::{
	game_session::GameSession,
	project::{BehaviorSpace, BehaviorSpec, MAX_PROJECT_BYTES, ProjectData},
	scene_catalog::{SceneDescriptor, SceneId},
	scene_data::SceneData,
};

#[derive(Clone, PartialEq, Eq)]
struct PackageAsset {
	relative: PathBuf,
	bytes: u64,
	fingerprint: [u8; 32],
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

/// Exported authoring data frozen before compilation. Finishing uses only this
/// output directory and never rereads the editable project's source assets.
#[derive(Clone)]
pub struct StagedProject {
	path: PathBuf,
	manifest_bytes: u64,
	manifest_fingerprint: [u8; 32],
	files: Arc<[PackageAsset]>,
	asset_bytes: u64,
}
impl StagedProject {
	pub fn path(&self) -> &Path {
		&self.path
	}
	pub fn asset_count(&self) -> usize {
		self.files.len()
	}
	pub fn asset_bytes(&self) -> u64 {
		self.asset_bytes
	}
}

/// A completed export and the fingerprint of its installed executable.
#[derive(Clone)]
pub struct InstalledProject {
	staged: StagedProject,
	player: PackageAsset,
}

impl InstalledProject {
	pub fn path(&self) -> &Path {
		self.staged.path()
	}

	pub fn verified_executable(&self) -> Result<PathBuf, String> {
		validate_export_snapshot(&self.staged, Some(&self.player.relative))?;
		let result = || -> io::Result<PathBuf> {
			let executable = checked_asset_path(self.path(), &self.player.relative)?;
			copy_verified_asset(&executable, &self.player, &mut io::sink())?;
			Ok(executable)
		};
		result().map_err(|error| format!("Built game changed or is unavailable: {error}"))
	}
}

const MAX_SCENE_REPORT_BYTES: usize = 16 * 1024;
const OMISSION_NOTICE: &str = "\nFurther scene diagnostics omitted.";

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

/// Create editable starter data without compiling or installing a game player.
pub fn create_authoring_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	project_path(name, destination)?;
	let (mut project, root) = bundled_starter(name)?;
	project.gameplay_module = None;
	let prepared = prepare_project(&project, &root)?;
	let path = project_path(&project.name, destination)?;
	prepared.validate_inputs()?;
	// Never merge starter data into an existing project, including a folder
	// created after the initial destination check.
	fs::create_dir(&path).map_err(|e| format!("Could not create project folder: {e}"))?;
	copy_prepared_project(&prepared, &path, &project_json(&prepared.project)?).map_err(
		|error| {
			format!(
				"Could not create project: {error}; incomplete output retained at {}",
				path.display()
			)
		},
	)?;
	Ok(path)
}

/// Compatibility entry point that builds and packages the bundled starter.
pub fn install_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	project_path(name, destination)?;
	let (project, root) = bundled_starter(name)?;
	install_authored_project(&project, &root, destination)
}

fn bundled_starter(name: &str) -> Result<(ProjectData, PathBuf), String> {
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
	Ok((project, root))
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
	finish_staged_project(stage_prepared_project(prepared, destination)?)
}

pub fn stage_prepared_project(
	prepared: &PreparedProject,
	destination: &str,
) -> Result<StagedProject, String> {
	let path = project_path(&prepared.project.name, destination)?;
	prepared.validate_inputs()?;
	let project_json = project_json(&prepared.project)?;
	let parent = path.parent().ok_or("Missing export parent folder")?;
	let name = path.file_name().ok_or("Missing export folder name")?;
	let path = parent.canonicalize().map_err(|e| e.to_string())?.join(name);
	// create_dir is atomic: never merge into an existing project.
	fs::create_dir(&path).map_err(|e| format!("Could not create project folder: {e}"))?;
	let result = (|| -> Result<(), std::io::Error> {
		copy_prepared_project(prepared, &path, &project_json)?;
		write_new(
			&path.join("project-name.txt"),
			prepared.project.name.as_bytes(),
		)?;
		write_new(
			&path.join("README.txt"),
			b"Game Engine project\nA data-only stage contains the project and assets without a player executable.\nA completed export also contains game.exe; run it to open the Vulkan game window.\nRequires a Vulkan-capable GPU and driver.\nScene JSON and its model, image, material and prefab assets are installed beneath assets.\nAuthoring belongs to the Angular/Tauri editor; keep the assets folder beside the executable.\n",
		)?;
		Ok(())
	})();
	if let Err(error) = result {
		// Retain the new folder for inspection; never recursively remove a path
		// that may have been modified by another process during a long build.
		return Err(format!(
			"Could not stage project: {error}; incomplete output retained at {}",
			path.display()
		));
	}
	Ok(StagedProject {
		path,
		manifest_bytes: project_json.len() as u64,
		manifest_fingerprint: Sha256::digest(project_json.as_bytes()).into(),
		files: prepared.files.clone().into(),
		asset_bytes: prepared.total_bytes(),
	})
}

pub fn finish_staged_project(staged: StagedProject) -> Result<PathBuf, String> {
	complete_staged_project(staged).map(|installed| installed.staged.path)
}

pub fn complete_staged_project(staged: StagedProject) -> Result<InstalledProject, String> {
	validate_export_snapshot(&staged, None)?;
	let executable = compile_player().map_err(|error| {
		format!(
			"{error}; incomplete output retained at {}",
			staged.path.display()
		)
	})?;
	install_staged_player(staged, &executable)
}

fn compile_player() -> Result<PathBuf, String> {
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
	Ok(target
		.join("release")
		.join(format!("game{}", std::env::consts::EXE_SUFFIX)))
}

fn check_export_folder(path: &Path) -> io::Result<()> {
	let metadata = fs::symlink_metadata(path)?;
	if !metadata.is_dir() || path.canonicalize()? != path {
		return Err(io::Error::other("Export folder changed"));
	}
	Ok(())
}

fn validate_export_snapshot(staged: &StagedProject, player: Option<&Path>) -> Result<(), String> {
	let check = || -> io::Result<()> {
		check_export_folder(&staged.path)?;
		for entry in fs::read_dir(&staged.path)? {
			let entry = entry?;
			let name = entry.file_name();
			let name = Path::new(&name);
			let expected = ["project.json", "project-name.txt", "README.txt", "assets"]
				.iter()
				.any(|expected| name == Path::new(expected));
			if !expected && player != Some(name) {
				return Err(io::Error::other("Unexpected file in export folder"));
			}
		}
		let manifest = checked_asset_path(&staged.path, Path::new("project.json"))?;
		let fingerprint = copy_and_hash(&manifest, staged.manifest_bytes, &mut io::sink())?;
		if fingerprint != staged.manifest_fingerprint {
			return Err(io::Error::other("Staged project.json changed"));
		}
		let root = staged.path.join("assets");
		check_export_folder(&root)?;
		if prepare_assets(&root)?.as_slice() != staged.files.as_ref() {
			return Err(io::Error::other("Staged assets changed"));
		}
		Ok(())
	};
	check().map_err(|error| format!("Export snapshot changed or is unavailable: {error}"))
}

fn install_staged_player(
	staged: StagedProject,
	executable: &Path,
) -> Result<InstalledProject, String> {
	validate_export_snapshot(&staged, None)?;
	let path = staged.path();
	let relative = PathBuf::from(format!("game{}", std::env::consts::EXE_SUFFIX));
	let destination = path.join(&relative);
	let install = || -> io::Result<PackageAsset> {
		static NEXT_PLAYER: AtomicU64 = AtomicU64::new(0);
		let temporary = path.join(format!(
			".game-{}-{}.tmp",
			std::process::id(),
			NEXT_PLAYER.fetch_add(1, Ordering::Relaxed)
		));
		let metadata = fs::metadata(executable)?;
		if !metadata.is_file() {
			return Err(io::Error::other("Compiled player must be a regular file"));
		}
		let mut writer = fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temporary)?;
		let result = (|| -> io::Result<PackageAsset> {
			let bytes = metadata.len();
			let fingerprint = copy_and_hash(executable, bytes, &mut writer)?;
			writer.set_permissions(metadata.permissions())?;
			writer.sync_all()?;
			drop(writer);
			publish_player(&temporary, &destination)?;
			Ok(PackageAsset {
				relative,
				bytes,
				fingerprint,
			})
		})();
		// Windows moves the temporary file. Unix publishes a second directory entry.
		let _ = fs::remove_file(&temporary);
		result
	};
	let player = install().map_err(|error| {
		format!(
			"Could not install game executable: {error}; incomplete output retained at {}",
			path.display()
		)
	})?;
	Ok(InstalledProject { staged, player })
}

#[cfg(windows)]
fn publish_player(source: &Path, destination: &Path) -> io::Result<()> {
	use std::os::windows::ffi::OsStrExt;
	use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

	let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
	let target: Vec<_> = destination
		.as_os_str()
		.encode_wide()
		.chain(Some(0))
		.collect();
	// Leaving the replace flag unset preserves an occupied destination.
	if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } == 0 {
		return Err(io::Error::last_os_error());
	}
	Ok(())
}

#[cfg(not(windows))]
fn publish_player(source: &Path, destination: &Path) -> io::Result<()> {
	fs::hard_link(source, destination)
}

fn project_json(project: &ProjectData) -> Result<String, String> {
	let mut json = project.to_json()?;
	if json.len() < MAX_PROJECT_BYTES {
		json.push('\n');
	}
	Ok(json)
}

fn copy_prepared_project(
	prepared: &PreparedProject,
	path: &Path,
	project_json: &str,
) -> io::Result<()> {
	copy_prepared_assets(&prepared.asset_root, &path.join("assets"), &prepared.files)?;
	let copied_root = path.join("assets").canonicalize()?;
	validate_catalog_scenes(&prepared.project, &copied_root, &prepared.files)
		.map_err(io::Error::other)?;
	write_new(&path.join("project.json"), project_json.as_bytes())
}

fn write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
	fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)?
		.write_all(contents)
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

fn verify_asset_fingerprint(asset: &PackageAsset, actual: [u8; 32]) -> io::Result<()> {
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

fn copy_and_hash(
	path: &Path,
	expected_bytes: u64,
	writer: &mut impl Write,
) -> io::Result<[u8; 32]> {
	let metadata = fs::symlink_metadata(path)?;
	if !metadata.is_file() {
		return Err(io::Error::other("Export input must be a regular file"));
	}
	if metadata.len() != expected_bytes {
		return Err(io::Error::other("Asset changed size after preflight"));
	}
	let file = fs::File::open(path)?;
	let metadata = file.metadata()?;
	if !metadata.is_file() || metadata.len() != expected_bytes {
		return Err(io::Error::other("Export input changed before reading"));
	}
	let mut reader = file.take(expected_bytes.saturating_add(1));
	let mut buffer = [0u8; 64 * 1024];
	let mut copied = 0u64;
	let mut fingerprint = Sha256::new();
	loop {
		let read = reader.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		copied += read as u64;
		if copied > expected_bytes {
			return Err(io::Error::other("Asset grew during installation"));
		}
		fingerprint.update(&buffer[..read]);
		writer.write_all(&buffer[..read])?;
	}
	if copied != expected_bytes {
		return Err(io::Error::other("Asset shrank during installation"));
	}
	Ok(fingerprint.finalize().into())
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
		for entry in fs::read_dir(path)? {
			collect_assets(root, &entry?.path(), depth + 1, files, total, visited)?;
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
	// Keep one validated project and target index for this immutable catalog pass.
	project.validate()?;
	let gameplay = if project.gameplay_module.is_some() {
		Some((
			Arc::new(project.clone()),
			courtyard_gameplay::resolve(project)?,
		))
	} else {
		None
	};
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
				if let Some((project, factory)) = &gameplay {
					GameSession::with_factory_for_scene(
						Arc::clone(project),
						entry.id().clone(),
						Arc::new(scene),
						Arc::clone(factory),
					)
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
