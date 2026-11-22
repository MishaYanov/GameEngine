//! Project gameplay registration shared by the Player, editor, exporter, and replays.

use std::sync::Arc;

use engine_core::{
	command::GameplayModuleDescriptorDto, project::ProjectData,
	simulation_session::GameModuleFactory,
};

pub use cinder_oath_gameplay::{self as cinder_oath, CINDER_OATH_ID};

pub const BUNDLED_PROJECT_DIRECTORY: &str = "projects/cinder-oath";

pub fn descriptors() -> Vec<GameplayModuleDescriptorDto> {
	vec![cinder_oath_gameplay::descriptor()]
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	match project.gameplay_module.as_deref() {
		Some(CINDER_OATH_ID) => cinder_oath_gameplay::resolve(project),
		// The legacy resolver still serves explicitly opened historical fixtures.
		_ => courtyard_gameplay::resolve(project),
	}
}
