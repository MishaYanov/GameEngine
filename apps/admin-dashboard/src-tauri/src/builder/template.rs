use super::{
	Builder, FileEntry, Generation, GenerationPreparation, PreparedGeneration, PreparedPreview,
	Preview, PreviewPreparation,
};
use crate::now_ms;
use std::{
	fs,
	hash::{Hash, Hasher},
	io::{self, Read},
	path::{Path, PathBuf},
};

const LIMIT: u64 = 512 * 1024 * 1024;
const PROJECT_MANIFEST: &str = "projects/cinder-oath/project.json";
const PROJECT_ASSETS: &str = "projects/cinder-oath/assets";

impl Builder {
	pub fn prepare_preview(request: PreviewPreparation) -> io::Result<PreparedPreview> {
		let PreviewPreparation {
			reservation,
			mut input,
		} = request;
		if input.name.is_empty()
			|| input.name.len() > 64
			|| !input
				.name
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
			|| !["dev", "release"].contains(&input.profile.as_str())
			|| input.target != "local"
		{
			return Err(io::Error::other(
				"Use a 1–64 character project name, dev/release profile and local target",
			));
		}
		let source = PathBuf::from(&input.source).canonicalize()?;
		let destination = PathBuf::from(&input.destination);
		if !destination.is_absolute() || destination.file_name().is_none() || destination.exists() {
			return Err(io::Error::other(
				"Destination must be a new absolute folder",
			));
		}
		let parent = destination
			.parent()
			.ok_or_else(|| io::Error::other("Missing destination parent"))?
			.canonicalize()?;
		let destination = parent.join(destination.file_name().unwrap());
		if destination.starts_with(&source) {
			return Err(io::Error::other(
				"Destination must be outside the source workspace",
			));
		}
		input.source = source.to_string_lossy().into_owned();
		input.destination = destination.to_string_lossy().into_owned();
		let mut files = vec![];
		let mut total = 0;
		for path in [
			"apps/game/Cargo.toml",
			"apps/game/src",
			PROJECT_ASSETS,
			"crates/logger/Cargo.toml",
			"crates/logger/src",
			"crates/renderer/Cargo.toml",
			"crates/renderer/build.rs",
			"crates/renderer/src",
			"crates/renderer/shaders",
			"crates/runtime/Cargo.toml",
			"crates/runtime/src",
			"crates/bridge/Cargo.toml",
			"crates/bridge/src",
			"crates/engine-core/Cargo.toml",
			"crates/engine-core/src",
			"crates/gameplay-catalog/Cargo.toml",
			"crates/gameplay-catalog/src",
			"crates/cinder-oath-gameplay/Cargo.toml",
			"crates/cinder-oath-gameplay/src",
		] {
			let path = source.join(path);
			walk(&source, &path, &mut files, &mut total)?;
		}
		let project = source.join(PROJECT_MANIFEST);
		if project.exists() {
			walk(&source, &project, &mut files, &mut total)?;
			validate_project_template(&project)?;
		}
		files.sort_by(|a, b| a.path.cmp(&b.path));
		let preview = Preview {
			id: reservation.id.clone(),
			input,
			files,
			total_bytes: total,
			excluded: vec![
				"admin dashboard, editor UI/host, compiler".into(),
				".git, .env, credentials, caches, target, node_modules, logs".into(),
				"unsupported asset formats and files outside template allowlist".into(),
			],
			template_version: 1,
		};
		Ok(PreparedPreview {
			reservation,
			preview,
		})
	}

	pub fn prepare_generation(request: GenerationPreparation) -> io::Result<PreparedGeneration> {
		let GenerationPreparation { reservation, plan } = request;
		let destination = PathBuf::from(&plan.input.destination);
		if destination.exists() {
			return Err(io::Error::other("Destination is occupied"));
		}
		let source = PathBuf::from(&plan.input.source);
		let staging = destination
			.parent()
			.unwrap()
			.join(format!(".engine-{}", plan.id));
		fs::create_dir(&staging)?;
		// Retain failed staging instead of recursively deleting user-visible paths.
		let result = (|| -> io::Result<Generation> {
			for file in &plan.files {
				let path = source.join(&file.path);
				let mut ancestor = source.clone();
				for part in Path::new(&file.path).components() {
					ancestor.push(part);
					if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
						return Err(io::Error::other("Template path became a symbolic link"));
					}
				}
				if !path.canonicalize()?.starts_with(&source) {
					return Err(io::Error::other("Template path escaped source"));
				}
				let bytes = read_file(&path)?;
				if fingerprint(&bytes) != file.fingerprint {
					return Err(io::Error::other(
						"Template changed after preview; preview again",
					));
				}
				let output = staging.join(&file.path);
				fs::create_dir_all(output.parent().unwrap())?;
				fs::write(output, bytes)?;
			}
			fs::write(
				staging.join("Cargo.toml"),
				"[workspace]\nresolver=\"2\"\nmembers=[\"apps/game\",\"crates/logger\",\"crates/renderer\",\"crates/runtime\",\"crates/bridge\",\"crates/engine-core\",\"crates/gameplay-catalog\",\"crates/cinder-oath-gameplay\"]\n",
			)?;
			fs::write(
				staging.join("README.md"),
				format!(
					"# {}\n\nNative game template v1. Requires Rust 1.89+ and Vulkan SDK (glslc).\n\nRun from this workspace with `cargo run -p game -- --player`.\nKeep the project manifest and its assets together under projects/cinder-oath.\nRuntime logging works independently of Engine Admin.\n",
					plan.input.name,
				),
			)?;
			let git_source = crate::comparable_path(&source)
				.to_string_lossy()
				.replace('\\', "/");
			let mut command = std::process::Command::new("git");
			command.args([
				"-c",
				&format!("safe.directory={git_source}"),
				"-C",
				&git_source,
				"rev-parse",
				"HEAD",
			]);
			crate::processes::hidden(&mut command);
			let revision = command
				.output()
				.ok()
				.filter(|o| o.status.success())
				.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
			let run = Generation {
				id: plan.id.clone(),
				name: plan.input.name.clone(),
				destination,
				staging: staging.clone(),
				source,
				state: "VALIDATING".into(),
				job_id: String::new(),
				detail: None,
				created_ms: now_ms(),
				finished_ms: None,
				revision,
				profile: plan.input.profile.clone(),
				target: plan.input.target.clone(),
				files: plan.files.clone(),
			};
			Ok(run)
		})();
		let run = result.map_err(|error| {
			io::Error::other(format!(
				"{error}; staging retained at {}",
				staging.display()
			))
		})?;
		Ok(PreparedGeneration {
			reservation,
			run,
			offline: plan.input.offline,
		})
	}
}

fn fingerprint(bytes: &[u8]) -> u64 {
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	bytes.hash(&mut hash);
	hash.finish()
}
fn read_file(path: &Path) -> io::Result<Vec<u8>> {
	let metadata = fs::symlink_metadata(path)?;
	if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > LIMIT {
		return Err(io::Error::other(
			"Unsupported template file or symbolic link",
		));
	}
	let mut bytes = vec![];
	fs::File::open(path)?
		.take(LIMIT + 1)
		.read_to_end(&mut bytes)?;
	if bytes.len() as u64 > LIMIT {
		return Err(io::Error::other("Template file is too large"));
	}
	Ok(bytes)
}
fn walk(root: &Path, path: &Path, files: &mut Vec<FileEntry>, total: &mut u64) -> io::Result<()> {
	let mut ancestor = root.to_path_buf();
	for component in path
		.strip_prefix(root)
		.map_err(io::Error::other)?
		.components()
	{
		ancestor.push(component);
		if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
			return Err(io::Error::other("Template path contains a symbolic link"));
		}
	}
	if !path.canonicalize()?.starts_with(root) {
		return Err(io::Error::other(
			"Template path escapes its source workspace",
		));
	}
	walk_bounded(root, path, files, total, 0, &mut 0)
}
fn walk_bounded(
	root: &Path,
	path: &Path,
	files: &mut Vec<FileEntry>,
	total: &mut u64,
	depth: usize,
	visited: &mut usize,
) -> io::Result<()> {
	*visited += 1;
	if depth > 32 || *visited > 20_000 {
		return Err(io::Error::other(
			"Template exceeds 32 directory levels or 20000 entries",
		));
	}
	let metadata = fs::symlink_metadata(path)?;
	if metadata.file_type().is_symlink() {
		return Err(io::Error::other("Template symbolic links are unsupported"));
	}
	if metadata.is_dir() {
		let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
		entries.sort_by_key(|entry| entry.file_name());
		for entry in entries {
			walk_bounded(root, &entry.path(), files, total, depth + 1, visited)?;
		}
	} else {
		let relative = path
			.strip_prefix(root)
			.map_err(io::Error::other)?
			.to_string_lossy()
			.replace('\\', "/");
		let extension = path
			.extension()
			.and_then(|e| e.to_str())
			.unwrap_or("")
			.to_ascii_lowercase();
		let project_json = relative == PROJECT_MANIFEST;
		let project_asset = relative
			.strip_prefix(PROJECT_ASSETS)
			.is_some_and(|suffix| suffix.starts_with('/'));
		let asset_json = project_asset && extension == "json";
		let allowed = relative.ends_with(".rs")
			|| relative.ends_with("Cargo.toml")
			|| relative.ends_with(".vert")
			|| relative.ends_with(".frag")
			|| project_json
			|| (project_asset
				&& [
					"png", "jpg", "jpeg", "obj", "gltf", "glb", "bin", "json", "wav",
				]
				.contains(&extension.as_str()));
		if allowed {
			if relative.len() > 1024 {
				return Err(io::Error::other("Template path is too long"));
			}
			if asset_json && metadata.len() > 16 * 1024 * 1024 {
				return Err(io::Error::other("Asset JSON exceeds 16 MiB"));
			}
			if project_json && metadata.len() > 1024 * 1024 {
				return Err(io::Error::other("Project JSON exceeds 1 MiB"));
			}
			let bytes = read_file(path)?;
			if asset_json || project_json {
				serde_json::from_slice::<serde_json::Value>(&bytes).map_err(io::Error::other)?;
			}
			*total = total
				.checked_add(bytes.len() as u64)
				.ok_or_else(|| io::Error::other("Template byte count overflow"))?;
			if *total > LIMIT || files.len() >= 10000 {
				return Err(io::Error::other("Template exceeds 512 MiB or 10000 files"));
			}
			files.push(FileEntry {
				path: relative,
				bytes: bytes.len() as u64,
				fingerprint: fingerprint(&bytes),
			});
		}
	}
	Ok(())
}
fn validate_project_template(manifest: &Path) -> io::Result<()> {
	let project = engine_core::project::ProjectData::load(manifest).map_err(io::Error::other)?;
	let root = manifest
		.parent()
		.ok_or_else(|| io::Error::other("Project manifest has no parent directory"))?
		.join("assets")
		.canonicalize()?;
	for entry in project.scenes.iter() {
		let mut path = root.clone();
		for component in Path::new(entry.asset_path()).components() {
			if !matches!(component, std::path::Component::Normal(_)) {
				return Err(io::Error::other(
					"Startup scene path must remain asset-relative",
				));
			}
			path.push(component);
			if fs::symlink_metadata(&path)?.file_type().is_symlink() {
				return Err(io::Error::other(
					"Startup scene symbolic links are unsupported",
				));
			}
		}
		if !path.canonicalize()?.starts_with(&root) {
			return Err(io::Error::other("Startup scene escaped the asset root"));
		}
		let file = fs::File::open(path)?;
		let metadata = file.metadata()?;
		if !metadata.is_file() || metadata.len() > engine_core::scene_data::MAX_SCENE_BYTES as u64 {
			return Err(io::Error::other(
				"Startup scene must be a regular file within the scene byte limit",
			));
		}
		let mut json = String::new();
		file.take(engine_core::scene_data::MAX_SCENE_BYTES as u64 + 1)
			.read_to_string(&mut json)?;
		let scene = engine_core::scene_data::parse_json(&json).map_err(io::Error::other)?;
		project
			.validate_scene_for(entry.id(), &scene.data)
			.map_err(io::Error::other)?;
	}
	Ok(())
}
