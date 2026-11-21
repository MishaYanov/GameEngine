//! Create editable starter projects and resolve project destinations.

use engine_core::{
	project::{BehaviorSpace, BehaviorSpec},
	scene_data::SceneData,
};

use super::{
	Path, PathBuf, ProjectData, copy_prepared_project, fs, install_authored_project,
	prepare_project, project_json, read_authored_scene,
};

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
	project.gameplay_settings = None;
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

pub(super) fn bundled_starter(name: &str) -> Result<(ProjectData, PathBuf), String> {
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
