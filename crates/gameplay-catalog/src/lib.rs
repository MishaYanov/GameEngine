//! Project gameplay registration shared by the Player, editor, exporter, and replays.

use std::sync::Arc;

use engine_core::{
	command::GameplayModuleDescriptorDto, project::ProjectData,
	simulation_session::{GameModuleFactory, resolve_default_gameplay},
};

pub use cinder_oath_gameplay::{self as cinder_oath, CINDER_OATH_ID};

pub const BUNDLED_PROJECT_DIRECTORY: &str = "projects/cinder-oath";

pub fn descriptors() -> Vec<GameplayModuleDescriptorDto> {
	vec![cinder_oath_gameplay::descriptor()]
}

pub fn script_descriptors() -> Vec<engine_core::components::ScriptDescriptor> {
	vec![cinder_oath_gameplay::object_script_descriptor()]
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	match project.gameplay_module.as_deref() {
		Some(CINDER_OATH_ID) => cinder_oath_gameplay::resolve(project),
		_ => resolve_default_gameplay(project),
	}
}
