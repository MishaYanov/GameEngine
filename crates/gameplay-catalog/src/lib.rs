//! Project gameplay registration shared by the Player, editor, exporter, and replays.

mod maze;
pub mod project_scripts;
mod rotation_routine;

use std::{path::Path, sync::Arc};

use engine_core::{
	command::GameplayModuleDescriptorDto,
	game_progress::ProgressState,
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{
		GameModule, GameModuleFactory, GameModuleResolver, ModuleSettingReferences,
		ObjectScriptRegistration, PreparedModuleConfig, ProjectScriptRegistration,
		resolve_default_gameplay,
	},
};

pub use cinder_oath_gameplay::{self as cinder_oath, CINDER_OATH_ID};

pub const BUNDLED_PROJECT_DIRECTORY: &str = "projects/cinder-oath";
const OBJECT_SCRIPTS: [ObjectScriptRegistration; 2] = [
	cinder_oath_gameplay::object_script_registration(),
	rotation_routine::registration(),
];

pub fn descriptors() -> Vec<GameplayModuleDescriptorDto> {
	vec![cinder_oath_gameplay::descriptor(), maze::descriptor()]
}

pub fn script_descriptors() -> Vec<engine_core::components::ScriptDescriptor> {
	OBJECT_SCRIPTS
		.iter()
		.map(|script| (script.descriptor)())
		.collect()
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	resolve_factory(project, None)
}

pub fn project_resolver(root: &Path, source: project_scripts::ScriptSource) -> GameModuleResolver {
	let root = root.to_path_buf();
	Arc::new(move |project| {
		resolve_factory(
			project,
			Some(project_scripts::ProjectScripts::new(
				root.clone(),
				source.clone(),
			)),
		)
	})
}

fn resolve_factory(
	project: &ProjectData,
	scripts: Option<project_scripts::ProjectScripts>,
) -> Result<Arc<dyn GameModuleFactory>, String> {
	let module = match project.gameplay_module.as_deref() {
		Some(CINDER_OATH_ID) => cinder_oath_gameplay::resolve(project),
		Some(maze::ID) => maze::resolve(project),
		_ => resolve_default_gameplay(project),
	}?;
	Ok(Arc::new(RegisteredFactory { module, scripts }))
}

struct RegisteredFactory {
	module: Arc<dyn GameModuleFactory>,
	scripts: Option<project_scripts::ProjectScripts>,
}

impl GameModuleFactory for RegisteredFactory {
	fn object_scripts(&self) -> &[ObjectScriptRegistration] {
		&OBJECT_SCRIPTS
	}

	fn project_scripts(&self) -> Result<Vec<ProjectScriptRegistration>, String> {
		self.scripts
			.as_ref()
			.map_or_else(|| Ok(Vec::new()), |scripts| scripts.registrations())
	}

	fn prefab_references(
		&self,
		settings: &serde_json::Value,
	) -> Result<ModuleSettingReferences, String> {
		self.module.prefab_references(settings)
	}

	fn create_progress(&self) -> Result<ProgressState, String> {
		self.module.create_progress()
	}

	fn create(
		&self,
		project: &ProjectData,
		scene: &SceneId,
	) -> Result<Box<dyn GameModule>, String> {
		self.module.create(project, scene)
	}

	fn create_prepared(
		&self,
		config: PreparedModuleConfig<'_>,
	) -> Result<Box<dyn GameModule>, String> {
		self.module.create_prepared(config)
	}
}
