use crate::{
	jobs::{Jobs, Kind, Status},
	now_ms, save_json,
};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	fs,
	hash::{Hash, Hasher},
	io::{self, Read},
	path::{Path, PathBuf},
};
const LIMIT: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
	pub name: String,
	pub source: String,
	pub destination: String,
	pub profile: String,
	pub target: String,
	#[serde(default = "offline_default")]
	pub offline: bool,
}
fn offline_default() -> bool {
	true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
	pub path: String,
	pub bytes: u64,
	pub fingerprint: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
	pub id: String,
	pub input: Input,
	pub files: Vec<FileEntry>,
	pub total_bytes: u64,
	pub excluded: Vec<String>,
	pub template_version: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
	pub id: String,
	pub name: String,
	pub destination: PathBuf,
	pub staging: PathBuf,
	pub source: PathBuf,
	pub state: String,
	pub job_id: String,
	pub detail: Option<String>,
	pub created_ms: u64,
	pub finished_ms: Option<u64>,
	pub revision: Option<String>,
	pub profile: String,
	pub target: String,
	#[serde(default)]
	pub files: Vec<FileEntry>,
}
pub struct Builder {
	pub runs: Vec<Generation>,
	pub storage_error: Option<String>,
	path: PathBuf,
	plans: BTreeMap<String, Preview>,
	sequence: u64,
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
		let project_json = relative == "apps/game/project.json";
		let asset_json = relative.starts_with("apps/game/assets/") && extension == "json";
		let allowed = relative.ends_with(".rs")
			|| relative.ends_with("Cargo.toml")
			|| relative.ends_with(".vert")
			|| relative.ends_with(".frag")
			|| project_json
			|| (relative.starts_with("apps/game/assets/")
				&& ["png", "jpg", "jpeg", "obj", "gltf", "glb", "bin", "json"]
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
fn publish_new(source: &Path, destination: &Path) -> io::Result<()> {
	#[cfg(windows)]
	{
		use std::os::windows::ffi::OsStrExt;
		let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
		let destination: Vec<u16> = destination
			.as_os_str()
			.encode_wide()
			.chain(Some(0))
			.collect();
		// No replacement flag: a concurrent destination creator wins safely.
		if unsafe {
			windows_sys::Win32::Storage::FileSystem::MoveFileExW(
				source.as_ptr(),
				destination.as_ptr(),
				0,
			)
		} == 0
		{
			return Err(io::Error::last_os_error());
		}
		Ok(())
	}
	#[cfg(not(windows))]
	{
		let _ = (source, destination);
		Err(io::Error::other(
			"Project publication currently supports Windows only",
		))
	}
}

fn validate_project_template(source: &Path, manifest: &Path) -> io::Result<()> {
	let project = engine_core::project::ProjectData::load(manifest).map_err(io::Error::other)?;
	let root = source.join("apps/game/assets").canonicalize()?;
	let mut path = root.clone();
	for component in Path::new(&project.startup_scene).components() {
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
		.validate_scene(&scene.data)
		.map_err(io::Error::other)
}
impl Builder {
	pub fn new(data: &Path) -> Self {
		let path = data.join("projects.json");
		let loaded = fs::File::open(&path).and_then(|f| {
			serde_json::from_reader::<_, Vec<Generation>>(f.take(32 * 1024 * 1024))
				.map_err(io::Error::other)
		});
		let (mut runs, storage_error) = match loaded {
			Ok(r) => (r, None),
			Err(e) if e.kind() == io::ErrorKind::NotFound => (vec![], None),
			Err(e) => (vec![], Some(e.to_string())),
		};
		runs.truncate(50);
		for run in &mut runs {
			if run.state == "VALIDATING" {
				run.state = "INTERRUPTED".into();
				run.detail = Some("Validation interrupted; staging retained for inspection".into());
				run.finished_ms = Some(now_ms());
			}
		}
		Self {
			runs,
			storage_error,
			path,
			plans: BTreeMap::new(),
			sequence: 0,
		}
	}
	fn persist(&mut self) {
		while self.runs.len() > 1
			&& serde_json::to_vec(&self.runs).is_ok_and(|bytes| bytes.len() > 32 * 1024 * 1024)
		{
			self.runs.pop();
		}
		self.storage_error = save_json(&self.path, &self.runs)
			.err()
			.map(|e| e.to_string());
	}
	pub fn preview(&mut self, mut input: Input) -> io::Result<Preview> {
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
			"apps/game/assets",
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
		] {
			let path = source.join(path);
			walk(&source, &path, &mut files, &mut total)?;
		}
		let project = source.join("apps/game/project.json");
		if project.exists() {
			walk(&source, &project, &mut files, &mut total)?;
			validate_project_template(&source, &project)?;
		}
		files.sort_by(|a, b| a.path.cmp(&b.path));
		self.sequence += 1;
		let preview = Preview {
			id: format!(
				"project-{}-{}-{}",
				std::process::id(),
				now_ms(),
				self.sequence
			),
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
		if self.plans.len() >= 16 {
			self.plans.clear();
		}
		self.plans.insert(preview.id.clone(), preview.clone());
		Ok(preview)
	}
	pub fn generate(&mut self, id: &str, jobs: &mut Jobs) -> io::Result<Generation> {
		if self.storage_error.is_some() {
			return Err(io::Error::other(
				"Recover project history storage before generating",
			));
		}
		if self.runs.iter().any(|r| r.state == "VALIDATING") {
			return Err(io::Error::other("A project is already being validated"));
		}
		let plan = self
			.plans
			.get(id)
			.cloned()
			.ok_or_else(|| io::Error::other("Preview expired; preview again"))?;
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
				"[workspace]\nresolver=\"2\"\nmembers=[\"apps/game\",\"crates/logger\",\"crates/renderer\",\"crates/runtime\",\"crates/bridge\",\"crates/engine-core\"]\n",
			)?;
			let main = staging.join("apps/game/src/main.rs");
			let text = fs::read_to_string(&main)?.replace("PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join(\"assets\")", "std::env::current_exe().expect(\"game executable path\").parent().expect(\"game executable folder\").join(\"../../apps/game/assets\")");
			fs::write(main, text)?;
			fs::write(
				staging.join("README.md"),
				format!(
					"# {}\n\nNative game template v1. Requires Rust 1.89+ and Vulkan SDK (glslc).\n\nRun from this workspace: `cargo run -p game`.\nAssets stay under apps/game/assets; retain the workspace layout when moving output.\nRuntime logging works independently of Engine Admin.\n",
					plan.input.name
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
			let job = jobs.queue(
				&staging,
				Kind::Build,
				"game".into(),
				plan.input.profile.clone(),
			)?;
			jobs.set_offline(&job.id, plan.input.offline);
			let run = Generation {
				id: plan.id.clone(),
				name: plan.input.name.clone(),
				destination,
				staging: staging.clone(),
				source,
				state: "VALIDATING".into(),
				job_id: job.id,
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
		match result {
			Ok(run) => {
				self.runs.insert(0, run.clone());
				self.runs.truncate(50);
				self.plans.remove(id);
				self.persist();
				Ok(run)
			}
			Err(e) => Err(io::Error::other(format!(
				"{e}; staging retained at {}",
				staging.display()
			))),
		}
	}
	pub fn tick(&mut self, jobs: &mut Jobs) {
		let mut changed = false;
		for run in &mut self.runs {
			if run.state != "VALIDATING" {
				continue;
			}
			let Some(job) = jobs.runs.iter().find(|j| j.id == run.job_id) else {
				run.state = "INTERRUPTED".into();
				run.detail = Some("Validation job no longer retained".into());
				changed = true;
				continue;
			};
			if matches!(job.state, Status::Queued | Status::Running) {
				continue;
			}
			run.finished_ms = Some(now_ms());
			changed = true;
			if job.state != Status::Succeeded {
				run.state = match job.state {
					Status::Cancelled => "CANCELLED",
					Status::Interrupted => "INTERRUPTED",
					_ => "FAILED",
				}
				.into();
				run.detail = Some("Validation did not succeed; staging retained".into());
				continue;
			}
			let artifacts: Vec<_> = job
				.artifacts
				.iter()
				.filter_map(|a| {
					crate::relative_to(Path::new(a), &run.staging)
						.map(|p| p.to_string_lossy().into_owned())
				})
				.collect();
			let manifest = serde_json::json!({"template_version":1,"name":run.name,"engine_revision":run.revision,"source_snapshot":"copied local working tree","profile":run.profile,"target":run.target,"operation_id":run.job_id,"toolchain":job.toolchain,"artifacts":artifacts,"files":run.files,"platform":format!("{}-{}",std::env::consts::ARCH,std::env::consts::OS),"created_ms":run.created_ms,"validated_ms":run.finished_ms});
			let result = (|| -> io::Result<()> {
				if run.destination.exists() {
					return Err(io::Error::other(
						"Destination became occupied; it was not overwritten",
					));
				}
				save_json(&run.staging.join("generation-manifest.json"), &manifest)?;
				publish_new(&run.staging, &run.destination)?;
				Ok(())
			})();
			match result {
				Ok(()) => {
					run.state = "SUCCEEDED".into();
					run.detail = Some("Validated independent workspace is ready".into());
				}
				Err(e) => {
					run.state = "FAILED".into();
					run.detail = Some(e.to_string());
				}
			}
		}
		if changed {
			self.persist();
		}
		for run in self.runs.iter().filter(|r| r.state == "SUCCEEDED") {
			jobs.relocate_artifacts(&run.job_id, &run.staging, &run.destination);
		}
	}
}
#[cfg(test)]
#[path = "tests_builder.rs"]
mod tests;
