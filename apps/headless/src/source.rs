use super::args::Options;
use engine_core::{
	project::{MAX_PROJECT_BYTES, ProjectData},
	scene_catalog::SceneId,
	scene_data::{MAX_SCENE_BYTES, SceneData, validate_model_path},
};
use sha2::{Digest, Sha256};
use std::{
	collections::HashSet,
	fs::File,
	io::Read,
	path::{Path, PathBuf},
};

pub(super) struct Source {
	pub(super) asset_root: PathBuf,
	pub(super) project: ProjectData,
	pub(super) parsed: engine_core::scene_data::ParsedScene,
	pub(super) documents: Vec<(SceneId, SceneData)>,
	pub(super) project_manifest_sha256: Option<String>,
}
pub(super) fn source(options: &Options) -> Result<Source, String> {
	if let Some(path) = &options.project {
		let manifest = path
			.canonicalize()
			.map_err(|e| format!("Cannot open project: {e}"))?;
		let manifest_text = read_bounded(&manifest, MAX_PROJECT_BYTES)?;
		let project = ProjectData::from_json(&manifest_text)?;
		// Custom asset roots are useful for diagnostics but cannot claim a package manifest identity.
		let project_manifest_sha256 = (options.replay.is_some() && options.asset_root.is_none())
			.then(|| format!("{:x}", Sha256::digest(manifest_text.as_bytes())));
		let asset_root = options
			.asset_root
			.clone()
			.unwrap_or_else(|| manifest.parent().unwrap_or(Path::new(".")).join("assets"));
		// Check the named directory before canonicalization erases its link metadata.
		bridge::feedback_audio::load(&asset_root)?;
		let root = asset_root
			.canonicalize()
			.map_err(|e| format!("Cannot open asset root: {e}"))?;
		let mut parsed = None;
		let mut documents = Vec::new();
		let mut paths = HashSet::new();
		let mut document_bytes = 0usize;
		for entry in project.scenes.iter() {
			let path = contained(&root, entry.asset_path())?;
			if !paths.insert(path.clone()) {
				return Err("Scene paths resolve to the same file".into());
			}
			let document =
				engine_core::scene_data::parse_json(&read_bounded(&path, MAX_SCENE_BYTES)?)?;
			project.validate_scene_for(entry.id(), &document.data)?;
			document_bytes = document_bytes
				.saturating_add(document.data.serialized_size()?.saturating_mul(4))
				.saturating_add(std::mem::size_of::<SceneData>());
			if document_bytes > engine_core::editor::EditorSession::MAX_RETAINED_DOCUMENT_BYTES {
				return Err("Retained scene document byte budget exceeded".into());
			}
			if entry.id() == &project.startup_scene_id {
				parsed = Some(document);
			} else {
				documents.push((entry.id().clone(), document.data));
			}
		}
		return Ok(Source {
			asset_root: root,
			project,
			parsed: parsed.ok_or("Startup scene is missing")?,
			documents,
			project_manifest_sha256,
		});
	}
	let scene = options
		.scene
		.as_ref()
		.ok_or("A scene or project is required")?
		.canonicalize()
		.map_err(|e| format!("Cannot open scene: {e}"))?;
	let default_root = scene
		.ancestors()
		.skip(1)
		.find(|path| path.file_name().is_some_and(|name| name == "assets"))
		.or_else(|| scene.parent())
		.ok_or("Scene has no parent folder")?;
	let root = options
		.asset_root
		.as_deref()
		.unwrap_or(default_root)
		.canonicalize()
		.map_err(|e| e.to_string())?;
	if !root.is_dir() {
		return Err("Asset root must be a directory".into());
	}
	let relative = scene
		.strip_prefix(&root)
		.map_err(|_| "Scene is outside the supplied asset root")?
		.to_str()
		.ok_or("Scene reference must be valid UTF-8")?
		.replace('\\', "/");
	let parsed = engine_core::scene_data::parse_json(&read_bounded(&scene, MAX_SCENE_BYTES)?)?;
	// A standalone run has no persisted catalog. Its transient identity must
	// remain stable so identical inputs produce comparable diagnostic hashes.
	Ok(Source {
		asset_root: root,
		project: ProjectData::for_scene_with_id(
			"Headless scene",
			SceneId::new("headless-scene")?,
			relative,
			&parsed.data,
		)?,
		parsed,
		documents: Vec::new(),
		project_manifest_sha256: None,
	})
}
fn contained(root: &Path, relative: &str) -> Result<PathBuf, String> {
	validate_model_path(relative)?;
	let path = root
		.join(relative)
		.canonicalize()
		.map_err(|e| format!("Cannot resolve startup scene: {e}"))?;
	if !path.starts_with(root) {
		return Err("Startup scene escapes its asset root".into());
	}
	Ok(path)
}
pub(super) fn read_bounded(path: &Path, limit: usize) -> Result<String, String> {
	let file = File::open(path).map_err(|e| format!("Cannot read '{}': {e}", path.display()))?;
	if !file.metadata().map_err(|e| e.to_string())?.is_file() {
		return Err("Inputs must be regular files".into());
	}
	let mut source = String::new();
	file.take(limit as u64 + 1)
		.read_to_string(&mut source)
		.map_err(|e| e.to_string())?;
	if source.len() > limit {
		return Err(format!(
			"Input '{}' exceeds its {limit}-byte limit",
			path.display()
		));
	}
	Ok(source)
}
