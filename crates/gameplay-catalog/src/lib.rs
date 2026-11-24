//! Project gameplay registration shared by the Player, editor, exporter, and replays.

mod rotation_routine;

use std::sync::Arc;

use engine_core::{
	command::GameplayModuleDescriptorDto,
	game_progress::ProgressState,
	project::ProjectData,
	scene_catalog::SceneId,
	simulation_session::{
		GameModule, GameModuleFactory, ModuleSettingReferences, ObjectScriptRegistration,
		PreparedModuleConfig, resolve_default_gameplay,
	},
};

pub use cinder_oath_gameplay::{self as cinder_oath, CINDER_OATH_ID};

pub const BUNDLED_PROJECT_DIRECTORY: &str = "projects/cinder-oath";
const OBJECT_SCRIPTS: [ObjectScriptRegistration; 2] = [
	cinder_oath_gameplay::object_script_registration(),
	rotation_routine::registration(),
];

pub fn descriptors() -> Vec<GameplayModuleDescriptorDto> {
	vec![cinder_oath_gameplay::descriptor()]
}

pub fn script_descriptors() -> Vec<engine_core::components::ScriptDescriptor> {
	OBJECT_SCRIPTS
		.iter()
		.map(|script| (script.descriptor)())
		.collect()
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	let module = match project.gameplay_module.as_deref() {
		Some(CINDER_OATH_ID) => cinder_oath_gameplay::resolve(project),
		_ => resolve_default_gameplay(project),
	}?;
	Ok(Arc::new(RegisteredFactory { module }))
}

struct RegisteredFactory {
	module: Arc<dyn GameModuleFactory>,
}

impl GameModuleFactory for RegisteredFactory {
	fn object_scripts(&self) -> &[ObjectScriptRegistration] {
		&OBJECT_SCRIPTS
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
