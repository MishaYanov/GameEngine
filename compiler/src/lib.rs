use std::{
	collections::BTreeSet,
	fs,
	io::{self, Read, Write},
	path::{Path, PathBuf},
	sync::Arc,
};

use engine_core::project::{MAX_PROJECT_BYTES, ProjectData};
use sha2::{Digest, Sha256};

mod dependencies;
mod export;
mod preflight;
mod scaffold;

pub use dependencies::verification::{
	DependencyOpen, DependencyOwner, DependencyProblem, DependencySeverity, DependencyTarget,
	inspect_scene_dependencies,
};
pub use export::{
	complete_staged_project, complete_staged_project_with_player, finish_staged_project,
	install_authored_project, install_prepared_project, install_project_from_manifest,
	stage_prepared_project,
};
pub use preflight::prepare_project;
pub use scaffold::{
	create_authoring_project, create_authoring_project_from_template, create_blank_project,
	install_project, project_path,
};

use export::{copy_prepared_project, validate_export_snapshot};
#[cfg(test)]
use preflight::read_authored_scene;
use preflight::validate_catalog_scenes;

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
	source_files: Vec<PackageAsset>,
	generated_files: std::collections::BTreeMap<PathBuf, Vec<u8>>,
	total_bytes: u64,
	catalog_source: Option<PackageAsset>,
	catalog_output: Option<Vec<u8>>,
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
		for asset in &self.source_files {
			checked_asset_path(&self.asset_root, &asset.relative)
				.and_then(|input| copy_verified_asset(&input, asset, &mut io::sink()))
				.map_err(|cause| {
					format!(
						"Prepared asset {:?} is no longer valid: {cause}",
						asset.relative
					)
				})?;
		}
		if let Some(asset) = &self.catalog_source {
			let input = checked_asset_path(&self.asset_root, &asset.relative)
				.map_err(|error| error.to_string())?;
			copy_verified_asset(&input, asset, &mut io::sink())
				.map_err(|error| format!("Asset catalog changed after preflight: {error}"))?;
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

fn project_json(project: &ProjectData) -> Result<String, String> {
	let mut json = project.to_json()?;
	if json.len() < MAX_PROJECT_BYTES {
		json.push('\n');
	}
	Ok(json)
}

fn write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
	fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)?
		.write_all(contents)
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
	let audio = bridge::feedback_audio::load(root).map_err(io::Error::other)?;
	let mut cue_files = BTreeSet::new();
	if let Some(audio) = audio {
		for (name, _) in audio.files().filter(|(name, _)| name.ends_with(".wav")) {
			let path = checked_asset_path(root, Path::new(name))?;
			cue_files.insert(
				path.strip_prefix(root)
					.map_err(io::Error::other)?
					.to_path_buf(),
			);
		}
	}
	if let Some(audio) = bridge::project_audio::load(root).map_err(io::Error::other)? {
		for (name, _) in audio.files().filter(|(name, _)| name.ends_with(".wav")) {
			let path = checked_asset_path(root, Path::new(name))?;
			cue_files.insert(
				path.strip_prefix(root)
					.map_err(io::Error::other)?
					.to_path_buf(),
			);
		}
	}
	scan_assets(root, &cue_files)
}

fn verify_frozen_asset_inventory(root: &Path, expected: &[PackageAsset]) -> io::Result<()> {
	// Preflight already validated cue waves and catalogued Audio Clips.
	let wave_files = expected
		.iter()
		.filter(|asset| {
			asset
				.relative
				.extension()
				.is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
		})
		.map(|asset| asset.relative.clone())
		.collect();
	if scan_assets(root, &wave_files)? != expected {
		return Err(io::Error::other("Staged assets changed"));
	}
	Ok(())
}

fn scan_assets(root: &Path, wave_files: &BTreeSet<PathBuf>) -> io::Result<Vec<PackageAsset>> {
	let mut files = Vec::new();
	let mut total = 0;
	collect_assets(root, root, 0, &mut files, &mut total, &mut 0, wave_files)?;
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
	wave_files: &BTreeSet<PathBuf>,
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
			collect_assets(
				root,
				&entry?.path(),
				depth + 1,
				files,
				total,
				visited,
				wave_files,
			)?;
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
	let relative = path.strip_prefix(root).map_err(io::Error::other)?;
	if !["png", "jpg", "jpeg", "obj", "gltf", "glb", "bin", "json"].contains(&extension.as_str())
		&& !(extension == "wav" && wave_files.contains(relative))
	{
		return Err(io::Error::other(format!(
			"Unsupported packaged asset format: {}",
			relative.display()
		)));
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
	if relative.as_os_str().len() > 1024 {
		return Err(io::Error::other("Asset path exceeds 1024 characters"));
	}
	files.push((relative.to_path_buf(), metadata.len()));
	Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/tests_lib.rs"]
mod tests;
