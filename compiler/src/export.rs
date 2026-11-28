//! Freeze project data, build the native player, and publish the verified export.

use std::{
	process::Command,
	sync::atomic::{AtomicU64, Ordering},
};

use super::{
	Digest, InstalledProject, PackageAsset, Path, PathBuf, PreparedProject, ProjectData, Sha256,
	StagedProject, checked_asset_path, copy_and_hash, copy_prepared_assets, fs, io,
	prepare_project, project_json, project_path, validate_catalog_scenes,
	verify_frozen_asset_inventory, write_new,
};

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
	engine_logger::debug!(target: "compiler::packaging", "Project staged: assets={}, bytes={}", prepared.asset_count(), prepared.total_bytes());
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
	engine_logger::debug!(target: "compiler::packaging", "Staged project verified; compiling player");
	let executable = compile_player().map_err(|error| {
		format!(
			"{error}; incomplete output retained at {}",
			staged.path.display()
		)
	})?;
	complete_staged_project_with_player(staged, &executable)
}

fn compile_player() -> Result<PathBuf, String> {
	let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.ok_or("Missing engine workspace")?;
	let target = std::env::var_os("CARGO_TARGET_DIR")
		.filter(|path| !path.is_empty())
		.map_or_else(|| workspace.join("target"), |path| workspace.join(path));
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

pub(super) fn validate_export_snapshot(
	staged: &StagedProject,
	player: Option<&Path>,
) -> Result<(), String> {
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
		verify_frozen_asset_inventory(&root, &staged.files)?;
		Ok(())
	};
	check().map_err(|error| format!("Export snapshot changed or is unavailable: {error}"))
}

/// Verify a staged export and publish a trusted, existing Player executable.
pub fn complete_staged_project_with_player(
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
			return Err(io::Error::other("Player must be a regular file"));
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
pub(super) fn publish_player(source: &Path, destination: &Path) -> io::Result<()> {
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
pub(super) fn publish_player(source: &Path, destination: &Path) -> io::Result<()> {
	fs::hard_link(source, destination)
}

pub(super) fn copy_prepared_project(
	prepared: &PreparedProject,
	path: &Path,
	project_json: &str,
) -> io::Result<()> {
	let source_files: Vec<_> = prepared
		.files
		.iter()
		.filter(|asset| {
			!prepared.generated_files.contains_key(&asset.relative)
				&& (prepared.catalog_output.is_none()
					|| asset.relative != Path::new(engine_core::asset_catalog::CATALOG_FILE))
		})
		.cloned()
		.collect();
	copy_prepared_assets(&prepared.asset_root, &path.join("assets"), &source_files)?;
	for (relative, bytes) in &prepared.generated_files {
		let output = path.join("assets").join(relative);
		fs::create_dir_all(
			output
				.parent()
				.ok_or_else(|| io::Error::other("Missing generated asset parent"))?,
		)?;
		write_new(&output, bytes)?;
	}
	if let Some(catalog) = &prepared.catalog_output {
		write_new(
			&path
				.join("assets")
				.join(engine_core::asset_catalog::CATALOG_FILE),
			catalog,
		)?;
	}
	let copied_root = path.join("assets").canonicalize()?;
	validate_catalog_scenes(&prepared.project, &copied_root, &prepared.files)
		.map_err(io::Error::other)?;
	write_new(&path.join("project.json"), project_json.as_bytes())
}
