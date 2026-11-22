//! Gameplay for projects that opt in to the Courtyard Run module.
use std::{collections::BTreeMap, sync::Arc};

use engine_core::{
	command::{GameplayModuleDescriptorDto, GameplayRoleDescriptorDto},
	project::ProjectData,
	simulation_session::{GameModuleFactory, resolve_default_gameplay},
};
use settings::InteractionSettings;

pub mod authoring_diagnostics;
mod chapters;
pub mod checkpoint;
mod preparation;
mod progress;
mod runtime;
pub mod settings;

pub use chapters::{CampaignDefinition, ChapterRun, CourseFacts};
pub use preparation::CampaignFactory;
pub use progress::{CampaignProgress, ProgressAction};

pub const COURTYARD_RUN_ID: &str = "courtyard_run";
const FINISH_SCENE_ID: &str = "finish";

pub fn descriptor() -> GameplayModuleDescriptorDto {
	GameplayModuleDescriptorDto {
		id: COURTYARD_RUN_ID.into(),
		label: "Courtyard Run".into(),
		description: "Collect a key, reach the exit, then complete the finish scene.".into(),
		default_roles: vec![
			GameplayRoleDescriptorDto {
				name: "player".into(),
				label: "Player".into(),
				requirement_hint: "Required when Key or Exit is assigned. Needs controlled movement with positive speed.".into(),
			},
			GameplayRoleDescriptorDto {
				name: "key".into(),
				label: "Key".into(),
				requirement_hint: "Assigning Key or Exit requires Player, Key, and Exit on different entities.".into(),
			},
			GameplayRoleDescriptorDto {
				name: "exit".into(),
				label: "Exit".into(),
				requirement_hint: "Assigning Key or Exit requires all three roles. Interaction Settings select a Finish destination; legacy scenes use scene ID finish.".into(),
			},
		],
		scene_roles: BTreeMap::from([(
			FINISH_SCENE_ID.into(),
			vec![
				GameplayRoleDescriptorDto {
					name: "player".into(),
					label: "Player".into(),
					requirement_hint: "Required in the finish scene. Needs controlled movement with positive speed.".into(),
				},
				GameplayRoleDescriptorDto {
					name: "finish".into(),
					label: "Finish".into(),
					requirement_hint: "Required in the finish scene and must reference a different entity from Player.".into(),
				},
			],
		)]),
	}
}

pub fn resolve(project: &ProjectData) -> Result<Arc<dyn GameModuleFactory>, String> {
	if project.gameplay_module.as_deref() == Some(COURTYARD_RUN_ID) {
		let settings = InteractionSettings::from_project(project)?;
		checkpoint::CheckpointRegistry::from_settings(project, settings.as_ref())?;
		let chaptered = settings
			.as_ref()
			.and_then(preparation::ChapteredFactory::new);
		return Ok(match chaptered {
			Some(factory) => Arc::new(factory),
			None => Arc::new(CampaignFactory),
		});
	}
	resolve_default_gameplay(project)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
