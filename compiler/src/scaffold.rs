//! Create editable starter projects and resolve project destinations.

use engine_core::{
	EntityKey,
	command::{EditorCommandDto, EditorCommandRequest},
	editor::EditorSession,
	scene_data::{CURRENT_SCHEMA_VERSION, LightData, LightKind, SceneData},
};

use super::{
	Path, PathBuf, ProjectData, copy_prepared_project, fs, install_authored_project,
	prepare_project, project_json, write_new,
};

const BLANK_SCENE_PATH: &str = "scenes/main.json";

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

/// Create a generic project with a Main scene, a Game Camera, and a directional light.
pub fn create_blank_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	let path = project_path(name, destination)?;
	let scene = SceneData {
		schema_version: CURRENT_SCHEMA_VERSION,
		bindings: Default::default(),
		entities: Vec::new(),
		camera: Default::default(),
		default_game_camera: None,
		environment: Default::default(),
		lights: vec![LightData::new(
			EntityKey::new("directional-light")?,
			"Directional Light",
			LightKind::Directional,
		)],
		organization: Default::default(),
	};
	let project = ProjectData::for_scene(name.trim(), BLANK_SCENE_PATH, &scene)?;
	let mut editor = EditorSession::for_project(scene, project)?;
	editor.execute(EditorCommandRequest {
		expected_revision: editor.revision(),
		command: EditorCommandDto::CreateCamera,
	})?;

	let scene_json = editor.authored_document().to_json()?;
	let manifest_json = project_json(editor.project())?;
	let scene_path = path.join("assets").join(BLANK_SCENE_PATH);
	let scene_directory = scene_path
		.parent()
		.ok_or("Blank scene path has no parent folder")?;
	write_project(path, |path| {
		fs::create_dir_all(scene_directory)?;
		write_new(&scene_path, scene_json.as_bytes())?;
		write_new(&path.join("project.json"), manifest_json.as_bytes())
	})
}

/// Create an editable copy of the Cinder Oath starter from the developer checkout.
pub fn create_authoring_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	create_authoring_project_from_template(name, destination, &source_template_manifest()?)
}

/// Create an editable project copy from an explicitly supplied template manifest.
pub fn create_authoring_project_from_template(
	name: &str,
	destination: &str,
	manifest: &Path,
) -> Result<PathBuf, String> {
	let path = project_path(name, destination)?;
	let (project, root) = load_template(name, manifest)?;
	let prepared = prepare_project(&project, &root)?;
	prepared.validate_inputs()?;
	let manifest_json = project_json(&prepared.project)?;
	write_project(path, |path| {
		copy_prepared_project(&prepared, path, &manifest_json)
	})
}

/// Compatibility entry point that builds and packages the bundled starter.
pub fn install_project(name: &str, destination: &str) -> Result<PathBuf, String> {
	project_path(name, destination)?;
	let (project, root) = load_template(name, &source_template_manifest()?)?;
	install_authored_project(&project, &root, destination)
}

fn source_template_manifest() -> Result<PathBuf, String> {
	let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.ok_or("Missing engine workspace")?;
	Ok(workspace
		.join(gameplay_catalog::BUNDLED_PROJECT_DIRECTORY)
		.join("project.json"))
}

fn load_template(name: &str, manifest: &Path) -> Result<(ProjectData, PathBuf), String> {
	let root = manifest
		.parent()
		.ok_or("Template manifest has no parent folder")?
		.join("assets");
	let mut project = ProjectData::load(manifest)?;
	project.name = name.trim().to_owned();
	Ok((project, root))
}

fn write_project(
	path: PathBuf,
	write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
	// Atomically claim the new folder before writing any project files.
	fs::create_dir(&path).map_err(|e| format!("Could not create project folder: {e}"))?;
	write(&path).map_err(|error| {
		format!(
			"Could not create project: {error}; incomplete output retained at {}",
			path.display()
		)
	})?;
	Ok(path)
}
